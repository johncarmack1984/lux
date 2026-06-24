//! ANSI E1.31 (sACN) multicast output — for network DMX nodes like the
//! DMXKing eDMX1 Pro.
//!
//! Unlike the Enttec Open DMX USB (a "dumb" FTDI chip the host bit-bangs the
//! DMX waveform through), an sACN node receives DMX *frames* over the network
//! and generates the waveform itself. We build a standard E1.31 data packet
//! (root + framing + DMP layers, full 512-slot universe) and send it to the
//! universe's well-known multicast group `239.255.{hi}.{lo}` on UDP 5568.
//!
//! Multicast means we never need the node's IP address — the eDMX1 Pro (which
//! defaults to DHCP) subscribes to the multicast group for its configured
//! universe, so this is genuinely zero-config beyond matching the universe.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicU8, Ordering};

use super::DmxSink;

/// E1.31 listens on UDP 5568 (ACN-defined).
const SACN_PORT: u16 = 5568;
/// Total packet length for a full 512-slot universe:
/// root layer (38) + framing layer (77) + DMP layer (523).
const PACKET_LEN: usize = 638;

/// An sACN/E1.31 multicast sender bound to a single DMX universe.
pub struct SacnSink {
    socket: UdpSocket,
    dest: SocketAddrV4,
    universe: u16,
    /// Sender component identifier — stable for the life of the process.
    cid: [u8; 16],
    /// Per-packet sequence number; receivers use it to spot lost/old packets.
    /// Wraps 255 -> 0 naturally via `AtomicU8`.
    sequence: AtomicU8,
}

impl SacnSink {
    /// Bind a sender for `universe` (1..=63999). When `interface` is given (a
    /// local NIC's IPv4), bind to it so multicast egresses that interface —
    /// needed on multi-homed machines (e.g. Wi-Fi + Ethernet) where the node
    /// hangs off a specific NIC. Otherwise bind `0.0.0.0` and let the OS route.
    pub fn new(universe: u16, interface: Option<Ipv4Addr>) -> Result<Self, String> {
        if universe == 0 || universe > 63999 {
            return Err(format!("sACN universe {universe} out of range (1..=63999)"));
        }
        let bind_ip = interface.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let socket = UdpSocket::bind((bind_ip, 0))
            .map_err(|e| format!("sACN socket bind to {bind_ip} failed: {e}"))?;
        // Universe N -> multicast group 239.255.{N>>8}.{N&0xff} (per E1.31).
        let group = Ipv4Addr::new(239, 255, (universe >> 8) as u8, (universe & 0xff) as u8);
        Ok(Self {
            socket,
            dest: SocketAddrV4::new(group, SACN_PORT),
            universe,
            cid: *uuid::Uuid::new_v4().as_bytes(),
            sequence: AtomicU8::new(0),
        })
    }
}

impl DmxSink for SacnSink {
    fn render(&self, channels: &[u8]) -> Result<(), String> {
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        let packet = build_packet(&self.cid, self.universe, channels, seq);
        self.socket
            .send_to(&packet, self.dest)
            .map(|_| ())
            .map_err(|e| format!("sACN send to {} failed: {e}", self.dest))
    }
}

/// Build one E1.31 data packet carrying `channels` at DMX slots 1.. of
/// `universe`. Free function (not a method) so the byte layout is testable
/// without binding a socket. Layout follows ANSI E1.31-2018 §4.1.
fn build_packet(cid: &[u8; 16], universe: u16, channels: &[u8], sequence: u8) -> [u8; PACKET_LEN] {
    let mut p = [0u8; PACKET_LEN];

    // ---- Root layer (ACN root, 38 bytes) ----
    p[0..2].copy_from_slice(&0x0010u16.to_be_bytes()); // preamble size
                                                       // [2..4] post-amble size = 0
    p[4..16].copy_from_slice(b"ASC-E1.17\0\0\0"); // ACN packet identifier
    p[16..18].copy_from_slice(&(0x7000u16 | (PACKET_LEN as u16 - 16)).to_be_bytes()); // flags+length
    p[18..22].copy_from_slice(&4u32.to_be_bytes()); // VECTOR_ROOT_E131_DATA
    p[22..38].copy_from_slice(cid);

    // ---- Framing layer (77 bytes) ----
    p[38..40].copy_from_slice(&(0x7000u16 | (PACKET_LEN as u16 - 38)).to_be_bytes()); // flags+length
    p[40..44].copy_from_slice(&2u32.to_be_bytes()); // VECTOR_E131_DATA_PACKET
    p[44..47].copy_from_slice(b"lux"); // source name (64 bytes, null-padded)
    p[108] = 100; // priority (0..=200, 100 = default)
                  // [109..111] synchronization address = 0 (unused)
    p[111] = sequence;
    // [112] options = 0 (not preview, not terminated)
    p[113..115].copy_from_slice(&universe.to_be_bytes());

    // ---- DMP layer (523 bytes) ----
    p[115..117].copy_from_slice(&(0x7000u16 | (PACKET_LEN as u16 - 115)).to_be_bytes()); // flags+length
    p[117] = 0x02; // VECTOR_DMP_SET_PROPERTY
    p[118] = 0xa1; // address type & data type
                   // [119..121] first property address = 0
    p[121..123].copy_from_slice(&1u16.to_be_bytes()); // address increment
    p[123..125].copy_from_slice(&513u16.to_be_bytes()); // property value count (start code + 512 slots)
                                                        // [125] DMX start code = 0
    let n = channels.len().min(512);
    p[126..126 + n].copy_from_slice(&channels[..n]); // slots 1.. carry the fixture channels

    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_layout_is_e131_conformant() {
        let cid = [0xABu8; 16];
        let channels = [10u8, 20, 30, 40, 50, 60];
        let p = build_packet(&cid, 1, &channels, 7);

        assert_eq!(p.len(), 638);
        // Root layer
        assert_eq!(p[0..2], [0x00, 0x10]); // preamble size = 16
        assert_eq!(p[2..4], [0x00, 0x00]); // post-amble size = 0
        assert_eq!(p[4..16], *b"ASC-E1.17\0\0\0"); // ACN packet identifier
        assert_eq!(p[16..18], [0x72, 0x6e]); // flags(0x7) + length 622
        assert_eq!(p[18..22], [0, 0, 0, 4]); // VECTOR_ROOT_E131_DATA
        assert_eq!(p[22..38], cid);
        // Framing layer
        assert_eq!(p[38..40], [0x72, 0x58]); // flags(0x7) + length 600
        assert_eq!(p[40..44], [0, 0, 0, 2]); // VECTOR_E131_DATA_PACKET
        assert_eq!(p[44..47], *b"lux"); // source name
        assert_eq!(p[108], 100); // priority
        assert_eq!(p[111], 7); // sequence
        assert_eq!(p[113..115], [0, 1]); // universe 1
        // DMP layer
        assert_eq!(p[115..117], [0x72, 0x0b]); // flags(0x7) + length 523
        assert_eq!(p[117], 0x02); // VECTOR_DMP_SET_PROPERTY
        assert_eq!(p[118], 0xa1); // address & data type
        assert_eq!(p[121..123], [0, 1]); // address increment
        assert_eq!(p[123..125], [0x02, 0x01]); // property value count = 513
        assert_eq!(p[125], 0x00); // DMX start code
        assert_eq!(p[126..132], channels); // fixture channels at slots 1..=6
        assert!(p[132..].iter().all(|&b| b == 0)); // remaining slots zeroed
    }

    #[test]
    fn universe_maps_to_multicast_group() {
        let sink = SacnSink::new(1, None).unwrap();
        assert_eq!(sink.dest, SocketAddrV4::new(Ipv4Addr::new(239, 255, 0, 1), 5568));
        // 300 = 0x012C -> 239.255.1.44
        let sink = SacnSink::new(300, None).unwrap();
        assert_eq!(sink.dest.ip(), &Ipv4Addr::new(239, 255, 1, 44));
    }

    #[test]
    fn rejects_out_of_range_universe() {
        assert!(SacnSink::new(0, None).is_err());
        assert!(SacnSink::new(64000, None).is_err());
    }

    #[test]
    fn sequence_increments_and_wraps() {
        let sink = SacnSink::new(1, None).unwrap();
        // fetch_add returns the prior value; first render uses 0, then 1, 2...
        assert_eq!(sink.sequence.fetch_add(1, Ordering::Relaxed), 0);
        assert_eq!(sink.sequence.fetch_add(1, Ordering::Relaxed), 1);
        sink.sequence.store(255, Ordering::Relaxed);
        assert_eq!(sink.sequence.fetch_add(1, Ordering::Relaxed), 255);
        assert_eq!(sink.sequence.load(Ordering::Relaxed), 0); // wrapped
    }
}
