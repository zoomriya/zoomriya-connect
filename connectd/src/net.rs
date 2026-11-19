//! The box's addresses on the home network, which its name points at.

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
