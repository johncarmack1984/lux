//! Art-Net device discovery (ArtPoll / ArtPollReply).
//!
//! sACN is a data-only protocol — it has no way to ask a node "what universe
//! are you on?". Art-Net does: an `ArtPoll` broadcast makes every node reply
//! with an `ArtPollReply` describing itself (IP, name, firmware, and the
//! universe each port is bound to). This is exactly how the DMXKing eDMX Config
//! tool finds devices — doing it here lets lux auto-target the eDMX's universe
//! and skip that tool entirely.
//!
//! Best-effort: if Art-Net is blocked, the port is busy, or nothing answers,
//! `discover` returns an empty list and the caller falls back to the configured
//! universe. Packet offsets follow Art-Net 4 and are validated against a real
//! eDMX1 PRO (see the Python prototype this was ported from).

// Parse-path hardening: this module reads attacker-shaped UDP packets, so raw
// slice indexing is denied here (the first step of the incremental
// `indexing_slicing` rollout). Every offset below indexes a fixed-size array
// proven long enough, or goes through `.get()`.
#![warn(clippy::indexing_slicing)]

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

const ARTNET_PORT: u16 = 6454;

/// A node that answered our `ArtPoll`.
#[derive(Debug, Clone)]
pub struct ArtNetNode {
    pub ip: Ipv4Addr,
    pub short_name: String,
    pub long_name: String,
    pub firmware: (u8, u8),
    /// True if output port A can drive DMX from the network (i.e. it listens for
    /// our sACN/Art-Net). Input-only ports are ignored for universe targeting.
    pub output: bool,
    net: u8,
    sub: u8,
    sw_out: u8,
}

impl ArtNetNode {
    /// 15-bit Art-Net Port-Address of output port A.
    pub fn port_address(&self) -> u16 {
        ((self.net as u16) << 8) | ((self.sub as u16) << 4) | self.sw_out as u16
    }

    /// sACN/E1.31 universe of output port A. DMXKing maps sACN universe N to
    /// Art-Net Port-Address N-1 ("sACN universe 1 = Art-Net 0:0:0").
    pub fn sacn_universe(&self) -> u16 {
        self.port_address() + 1
    }
}

/// The 14-byte `ArtPoll`: ID + OpCode 0x2000 + ProtVer 14 + TalkToMe + Priority.
fn build_artpoll() -> [u8; 14] {
    let mut p = [0u8; 14];
    p[0..8].copy_from_slice(b"Art-Net\0");
    p[8..10].copy_from_slice(&0x2000u16.to_le_bytes()); // OpCode is little-endian on the wire
    p[11] = 14; // ProtVerLo (Hi=0); TalkToMe + Priority stay 0 (broadcast reply)
    p
}

/// Parse an `ArtPollReply`. Returns `None` for anything that isn't one.
fn parse_reply(data: &[u8]) -> Option<ArtNetNode> {
    // Pin the read window to a compile-time length: every offset below then
    // indexes a known-200-byte array rather than an attacker-sized slice, so a
    // short or truncated packet is rejected here instead of panicking. SwOut[0]
    // at offset 190 is the last field read.
    let data: &[u8; 200] = data.get(..200)?.try_into().ok()?;
    if !data.starts_with(b"Art-Net\0") {
        return None;
    }
    if u16::from_le_bytes([data[8], data[9]]) != 0x2100 {
        return None;
    }
    let cstr = |b: &[u8]| {
        // Bytes up to the first NUL (or all of them if unterminated) — the
        // first `split` segment, which is always present.
        let name = b.split(|&c| c == 0).next().unwrap_or(b);
        String::from_utf8_lossy(name).into_owned()
    };
    Some(ArtNetNode {
        ip: Ipv4Addr::new(data[10], data[11], data[12], data[13]),
        firmware: (data[16], data[17]),
        net: data[18] & 0x7f,
        sub: data[19] & 0x0f,
        short_name: cstr(data.get(26..44)?),
        long_name: cstr(data.get(44..108)?),
        output: data[174] & 0x80 != 0, // PortTypes[0] bit7 = "can output from network"
        sw_out: data[190] & 0x0f,      // SwOut[0] low nibble
    })
}

fn bind() -> std::io::Result<UdpSocket> {
    // Bind 0.0.0.0:6454 so broadcast replies are received; enable SO_BROADCAST
    // so we can send the poll to the limited broadcast address.
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, ARTNET_PORT))?;
    socket.set_broadcast(true)?;
    Ok(socket)
}

/// Broadcast an `ArtPoll` and collect replies for up to `timeout`, returning
/// early once replies go quiet. The poll egresses the OS-routed interface (the
/// wired NIC, on the confirmed setup); replies broadcast back to UDP 6454.
pub fn discover(timeout: Duration) -> Vec<ArtNetNode> {
    let socket = match bind() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Art-Net discovery: bind to :{ARTNET_PORT} failed ({e}); skipping");
            return Vec::new();
        }
    };
    let dest = SocketAddrV4::new(Ipv4Addr::BROADCAST, ARTNET_PORT);
    let poll = build_artpoll();
    let _ = socket.set_read_timeout(Some(Duration::from_millis(150)));
    if let Err(e) = socket.send_to(&poll, dest) {
        log::warn!("Art-Net discovery: poll send failed ({e})");
    }

    let mut nodes: Vec<ArtNetNode> = Vec::new();
    let start = Instant::now();
    let mut last_new = start;
    let mut last_poll = Instant::now();
    let mut buf = [0u8; 1024];

    while start.elapsed() < timeout {
        // Art-Net nodes reply with a random delay (up to ~1s, to avoid reply
        // storms), so keep re-polling until one answers rather than giving up.
        if nodes.is_empty() && last_poll.elapsed() >= Duration::from_millis(500) {
            let _ = socket.send_to(&poll, dest);
            last_poll = Instant::now();
        }
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            if let Some(node) = buf.get(..n).and_then(parse_reply) {
                if !nodes.iter().any(|x| x.ip == node.ip) {
                    nodes.push(node);
                    last_new = Instant::now();
                }
            }
        }
        // Once something answered and it's gone quiet, stop early to keep startup snappy.
        if !nodes.is_empty() && last_new.elapsed() >= Duration::from_millis(400) {
            break;
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_reply() -> Vec<u8> {
        let mut p = vec![0u8; 214];
        p[0..8].copy_from_slice(b"Art-Net\0");
        p[8..10].copy_from_slice(&0x2100u16.to_le_bytes());
        p[10..14].copy_from_slice(&[192, 168, 1, 111]);
        p[16] = 3;
        p[17] = 12;
        p[18] = 0; // net
        p[19] = 0; // sub
        p[26..35].copy_from_slice(b"edmx1-pro");
        p[44..65].copy_from_slice(b"DMXking.com eDMX1 PRO");
        p[174] = 0xc0; // PortTypes[0]: output + input capable
        p[190] = 0; // SwOut[0]
        p
    }

    #[test]
    fn parses_real_edmx_reply_shape() {
        let node = parse_reply(&sample_reply()).expect("valid reply");
        assert_eq!(node.ip, Ipv4Addr::new(192, 168, 1, 111));
        assert_eq!(node.firmware, (3, 12));
        assert_eq!(node.short_name, "edmx1-pro");
        assert_eq!(node.long_name, "DMXking.com eDMX1 PRO");
        assert!(node.output);
        assert_eq!(node.sacn_universe(), 1); // Net 0 / Sub 0 / Uni 0 -> sACN universe 1
    }

    #[test]
    fn universe_math_matches_artnet_offset() {
        let node = |net: u8, sub: u8, sw_out: u8| ArtNetNode {
            ip: Ipv4Addr::UNSPECIFIED,
            short_name: String::new(),
            long_name: String::new(),
            firmware: (0, 0),
            output: true,
            net,
            sub,
            sw_out,
        };
        assert_eq!(node(0, 0, 0).sacn_universe(), 1); // first universe
        assert_eq!(node(0, 0, 3).sacn_universe(), 4); // eDMX4 default top port
        assert_eq!(node(1, 0, 0).sacn_universe(), 257); // Net 1 -> Port-Address 256
    }

    #[test]
    fn rejects_non_replies() {
        assert!(parse_reply(b"too short").is_none());
        let mut wrong_op = sample_reply();
        wrong_op[8..10].copy_from_slice(&0x2000u16.to_le_bytes()); // an ArtPoll, not a reply
        assert!(parse_reply(&wrong_op).is_none());
    }

    #[test]
    fn artpoll_is_well_formed() {
        let p = build_artpoll();
        assert_eq!(&p[0..8], b"Art-Net\0");
        assert_eq!(u16::from_le_bytes([p[8], p[9]]), 0x2000); // OpCode ArtPoll
        assert_eq!(p[11], 14); // ProtVerLo
    }

    #[test]
    #[ignore = "hits the LAN; run with `--ignored --nocapture` against a real Art-Net node"]
    fn live_discover() {
        let nodes = discover(Duration::from_millis(3000));
        for n in &nodes {
            println!(
                "{} '{}' fw {}.{} -> sACN universe {}",
                n.ip,
                n.short_name,
                n.firmware.0,
                n.firmware.1,
                n.sacn_universe()
            );
        }
        assert!(!nodes.is_empty(), "no Art-Net nodes answered on the LAN");
    }
}
