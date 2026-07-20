//! Outbound-address selection shared by the pairing rendezvous on both ends
//! (the headless node and the app).
//!
//! The claim-code pairing flow (docs/claim-code-pairing.md) matches an unpaired
//! box to the owner's account by *shared public network* — a NAT-proximity
//! trick. IPv4 makes that easy: every device behind a home NAT egresses from the
//! one public address. IPv6 does not: there's no NAT, so each device has its own
//! /128, and the box and the owner's phone only share their **/64** (the auth
//! service keys the rendezvous on the /64 to cope). But that only lines up when
//! both ends chose the *same* address family; on a dual-stack network where one
//! side happens onto IPv4 and the other onto IPv6 (e.g. WiFi with flaky IPv6 but
//! a wired box on clean IPv6), their keys — an IPv4 NAT address vs an IPv6 /64 —
//! can't be correlated and pairing silently never lists the box.
//!
//! So the two ends agree to **prefer IPv4** for pairing calls: the shared NAT
//! address is the one identity every device on a household reliably presents.
//! IPv6 is kept as a fallback (sorted after) so v6-only networks still connect
//! and match on the /64. This is the outbound half of that agreement — resolve a
//! host with IPv4 candidates first — used by both ends via reqwest's
//! `resolve_to_addrs`.

use std::net::SocketAddr;
use std::net::ToSocketAddrs;

/// Resolve `host:port`, IPv4 addresses first. Empty on resolution failure — the
/// caller then keeps reqwest's default (dual-stack) DNS, so a transient lookup
/// hiccup never turns into a hard pairing failure.
pub fn ipv4_first_addrs(host: &str, port: u16) -> Vec<SocketAddr> {
    let mut addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map(Iterator::collect)
        .unwrap_or_default();
    sort_ipv4_first(&mut addrs);
    addrs
}

/// Stable partition: IPv4 before IPv6, order within each family preserved (so a
/// multi-homed host keeps the resolver's own ordering among same-family addrs).
fn sort_ipv4_first(addrs: &mut [SocketAddr]) {
    addrs.sort_by_key(SocketAddr::is_ipv6);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn ipv4_sorts_ahead_of_ipv6_stably() {
        let v6a = SocketAddr::from((
            Ipv6Addr::new(0x2603, 0x8003, 0x1cf0, 0x88a0, 0, 0, 0, 1),
            443,
        ));
        let v4a = SocketAddr::from((Ipv4Addr::new(76, 33, 40, 136), 443));
        let v6b = SocketAddr::from((Ipv6Addr::LOCALHOST, 443));
        let v4b = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 85), 443));
        let mut addrs = vec![v6a, v4a, v6b, v4b];
        sort_ipv4_first(&mut addrs);
        // Both IPv4 first, then both IPv6 — and relative order within each family
        // is preserved (v4a before v4b, v6a before v6b).
        assert_eq!(addrs, vec![v4a, v4b, v6a, v6b]);
    }

    #[test]
    fn all_one_family_is_untouched() {
        let only_v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, 443));
        let mut addrs = vec![only_v6];
        sort_ipv4_first(&mut addrs);
        assert_eq!(addrs, vec![only_v6]);
    }
}
