//! The box's addresses: on the home network, and its stable IPv6 ones.

use std::net::IpAddr;

use zoomriya_proto::{HomeAddrs, addrs};

/// Every home-network address on the box's network ports, sorted, at most
/// four of each kind.
pub fn home_addrs() -> HomeAddrs {
    let mut home = HomeAddrs::default();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        if iface.is_loopback() {
            continue;
        }
        match iface.ip() {
            IpAddr::V4(a) if addrs::is_home(IpAddr::V4(a)) => home.ipv4.push(a),
            IpAddr::V6(a) if addrs::is_home(IpAddr::V6(a)) => home.ipv6.push(a),
            _ => {}
        }
    }
    home.ipv4.sort();
    home.ipv4.dedup();
    home.ipv4.truncate(4);
    home.ipv6.sort();
    home.ipv6.dedup();
    home.ipv6.truncate(4);
    home
}

/// The box's stable global IPv6 addresses: not the temporary privacy ones,
/// not deprecated or still being checked (Linux's /proc/net/if_inet6).
/// Empty where that can't be read.
pub fn stable_ipv6() -> std::collections::HashSet<std::net::Ipv6Addr> {
    // Through /proc/self: the unit's ProcSubset=pid hides /proc/net.
    std::fs::read_to_string("/proc/self/net/if_inet6")
        .map(|t| parse_if_inet6(&t))
        .unwrap_or_default()
}

/// Temporary, deprecated, tentative (linux/if_addr.h).
const UNSTABLE: u32 = 0x01 | 0x20 | 0x40;

fn parse_if_inet6(text: &str) -> std::collections::HashSet<std::net::Ipv6Addr> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let (hex, scope, flags) = (f.first()?, f.get(3)?, f.get(4)?);
            let flags = u32::from_str_radix(flags, 16).ok()?;
            // Scope 00: global.
            if *scope != "00" || flags & UNSTABLE != 0 || hex.len() != 32 {
                return None;
            }
            let n = u128::from_str_radix(hex, 16).ok()?;
            Some(std::net::Ipv6Addr::from(n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ipv6_leaves_out_temporary_ones() {
        let text = "\
20010db8000000000000000000000007 02 40 00 00 eth0
20010db800000000000000000000beef 02 40 00 01 eth0
fe800000000000000000000000000001 02 40 20 80 eth0
00000000000000000000000000000001 01 80 10 80 lo
20010db8000000000000000000000009 02 40 00 20 eth0
";
        let got = parse_if_inet6(text);
        assert_eq!(got, ["2001:db8::7".parse().expect("ip")].into());
    }
}
