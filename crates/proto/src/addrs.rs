//! Which addresses may go where in DNS.
//!
//! A box name points at the box on the home network, never at the internet:
//! otherwise anyone could register a name with a real padlock and point it at
//! any website. The guests' door is the opposite: a public address, and only
//! the box's own.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// An address on a home network, which a box name may point at:
/// 10/8, 172.16/12, 192.168/16, or an IPv6 unique local address (fc00::/7).
pub fn is_home(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(a) => a.is_private(),
        IpAddr::V6(a) => (a.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// A public internet address, which the guests' door may point at.
pub fn is_public(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(a) => is_public_v4(a),
        IpAddr::V6(a) => is_public_v6(a),
    }
}

fn is_public_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_private()
        || a.is_loopback()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_documentation()
        || a.is_unspecified()
        || a.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // shared address space (CGNAT)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // benchmarking
        || o[0] >= 240)
}

fn is_public_v6(a: Ipv6Addr) -> bool {
    // Global unicast is 2000::/3, minus documentation (2001:db8::/32).
    let s = a.segments();
    (s[0] & 0xe000) == 0x2000 && !(s[0] == 0x2001 && s[1] == 0x0db8)
}

/// Whether a door address belongs to the home the request came from: the
/// same IPv4 address, or the same IPv6 /56 (the smallest block most providers
/// give a home).
pub fn same_home(door: IpAddr, from: IpAddr) -> bool {
    match (door, from) {
        (IpAddr::V4(d), IpAddr::V4(f)) => d == f,
        (IpAddr::V6(d), IpAddr::V6(f)) => {
            let (d, f) = (d.octets(), f.octets());
            d[..7] == f[..7]
        }
        (IpAddr::V4(d), IpAddr::V6(f)) => f.to_ipv4_mapped() == Some(d),
        (IpAddr::V6(_), IpAddr::V4(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("address")
    }

    #[test]
    fn home_addresses() {
        for a in ["192.168.1.20", "10.0.0.5", "172.16.3.4", "fd12:3456::1"] {
            assert!(is_home(ip(a)), "{a}");
            assert!(!is_public(ip(a)), "{a}");
        }
        for a in ["8.8.8.8", "100.64.1.1", "127.0.0.1", "fe80::1", "2a01::1"] {
            assert!(!is_home(ip(a)), "{a}");
        }
    }

    #[test]
    fn public_addresses() {
        for a in ["81.2.69.160", "2a01:4f8::1"] {
            assert!(is_public(ip(a)), "{a}");
        }
        for a in [
            "100.64.0.1",
            "169.254.1.1",
            "0.1.2.3",
            "240.0.0.1",
            "198.18.0.1",
            "192.0.2.1",
            "2001:db8::1",
            "::1",
            "fe80::1",
            "fd00::1",
            "ff02::1",
        ] {
            assert!(!is_public(ip(a)), "{a}");
        }
    }

    #[test]
    fn door_must_be_home() {
        assert!(same_home(ip("81.2.69.160"), ip("81.2.69.160")));
        assert!(!same_home(ip("81.2.69.161"), ip("81.2.69.160")));
        assert!(same_home(
            ip("2a01:4f8:1:2a00::99"),
            ip("2a01:4f8:1:2aff::1")
        ));
        assert!(!same_home(
            ip("2a01:4f8:1:2b00::99"),
            ip("2a01:4f8:1:2a00::1")
        ));
        assert!(same_home(ip("81.2.69.160"), ip("::ffff:81.2.69.160")));
        assert!(!same_home(ip("2a01:4f8::1"), ip("81.2.69.160")));
    }
}
