//! Being reached from away, with no server (docs/direct-plus.md §3, §7):
//! which of the box's addresses work from outside, published in the box's
//! sealed record in the DHT, and what the home's network is like, for Box
//! Desktop.
//!
//! | Address | Published when |
//! |---|---|
//! | Public IPv6, stable (not the temporary privacy ones) | Always: first, as IPv6 often works where IPv4 cannot |
//! | The port the router opened for the box (UPnP, PCP, NAT-PMP) | When the router allowed it and its own outside address is public |
//! | The home's outside IPv4, as the DHT saw it, at the box's port | When the router opened nothing: works if someone forwarded the port |
//!
//! A router whose own outside address is private, shared (100.64/10) or not
//! what the internet sees sits behind a second NAT, usually the provider's
//! CGNAT: no IPv4 address of the box can be reached then, and none is
//! published.
//!
//! Home-network addresses are never published: devices learn them at home.

use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4},
    num::NonZeroU16,
    sync::Arc,
    time::Duration,
};

use iroh::Endpoint;
use serde::Serialize;
use tracing::{info, warn};
use zoomriya_keys::record::{MAX_ADDRS, RecordSecret};
use zoomriya_link::dht::Dht;
use zoomriya_proto::addrs::is_public;

use crate::{daemon::Ctx, net};

/// How often the record is published even when nothing changed: DHT nodes
/// keep it for about two hours.
const REPUBLISH: u64 = 30 * 60;
/// How often the addresses are looked at.
const LOOK: Duration = Duration::from_secs(60);

/// What the box knows about being reached from away.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Reach {
    /// The home's outside IPv4 address, as the DHT's nodes saw it.
    pub outside_ipv4: Option<Ipv4Addr>,
    /// The address and port the router opened for the box.
    pub port_mapped: Option<SocketAddr>,
    /// The box's stable public IPv6 addresses.
    pub ipv6: Vec<Ipv6Addr>,
    /// A second NAT before the internet (the provider's CGNAT, or a router
    /// behind another): `None` when the router did not say.
    pub double_nat: Option<bool>,
    /// What the record holds.
    pub published: Vec<SocketAddr>,
    /// When it was last published.
    pub published_at: Option<u64>,
}

/// 100.64.0.0/10, the providers' shared space (CGNAT).
fn is_shared(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (o[1] & 0xc0) == 64
}

/// What to publish, from iroh's addresses for the box (for IPv6), its stable
/// IPv6 addresses (empty: all count as stable), the router's mapping and the
/// outside IPv4 the DHT saw.
pub fn plan(
    addrs: &[SocketAddr],
    stable: &HashSet<Ipv6Addr>,
    mapped: Option<SocketAddrV4>,
    outside: Option<Ipv4Addr>,
    port: u16,
) -> Reach {
    let mut r = Reach {
        outside_ipv4: outside,
        ..Reach::default()
    };
    for a in addrs {
        if let IpAddr::V6(ip) = a.ip()
            && is_public(a.ip())
            && (stable.is_empty() || stable.contains(&ip))
            && !r.ipv6.contains(&ip)
        {
            r.ipv6.push(ip);
            r.published.push(*a);
        }
    }
    r.double_nat = mapped.map(|m| {
        let ip = *m.ip();
        is_shared(ip) || !is_public(IpAddr::V4(ip)) || outside.is_some_and(|o| o != ip)
    });
    match (mapped, r.double_nat) {
        (Some(m), Some(false)) => {
            r.port_mapped = Some(SocketAddr::V4(m));
            r.published.push(SocketAddr::V4(m));
        }
        // Nothing opened: the outside address works if someone forwarded
        // the port by hand.
        (None, _) => {
            if let Some(ip) = outside {
                r.published.push(SocketAddr::from((ip, port)));
            }
        }
        _ => {}
    }
    r.published.truncate(MAX_ADDRS);
    r
}

/// Keeps the record published, and the box's reach up to date for Box
/// Desktop, until the task is dropped.
pub async fn keep_published(ctx: Arc<Ctx>, endpoint: Endpoint, record: RecordSecret) {
    let dht = match Dht::start() {
        Ok(d) => d,
        Err(e) => {
            warn!("the DHT: {e:#}");
            return;
        }
    };
    // The router's mapping for the box's port, for iroh's UDP.
    let ports = portmapper::Client::new(portmapper::Config::default());
    if let Some(p) = NonZeroU16::new(ctx.cfg.port) {
        ports.update_local_port(p);
    }
    let mut last: Vec<SocketAddr> = Vec::new();
    let mut last_at = 0u64;
    loop {
        ports.procure_mapping();
        let mapped = *ports.watch_external_address().borrow();
        let outside = dht.outside().await;
        let addrs: Vec<SocketAddr> = endpoint.addr().ip_addrs().copied().collect();
        tracing::debug!(?addrs, ?mapped, ?outside, "the box's addresses");
        let mut reach = plan(
            &addrs,
            &net::stable_ipv6(),
            mapped,
            outside.ipv4.map(|a| *a.ip()),
            ctx.cfg.port,
        );
        let now = crate::now();
        if !ctx.away.on() {
            // Remote access is off: nothing to find from away.
            last.clear();
            last_at = 0;
            reach.published.clear();
        } else if !reach.published.is_empty()
            && (reach.published != last || now >= last_at + REPUBLISH)
        {
            match dht.publish(&record, &reach.published, now).await {
                Ok(()) => {
                    if reach.published != last {
                        info!(addrs = ?reach.published, "record published");
                    }
                    last = reach.published.clone();
                    last_at = now;
                }
                Err(e) => warn!("publishing the record: {e:#}"),
            }
        }
        reach.published_at = (last_at > 0).then_some(last_at);
        if let Ok(mut r) = ctx.reach.lock() {
            *r = reach;
        }
        tokio::time::sleep(LOOK).await;
    }
}

#[cfg(test)]
mod tests {
    //! Public addresses from ordinary ranges: documentation ranges are
    //! not public, so they would be left out.
    use super::*;

    fn a(s: &str) -> SocketAddr {
        s.parse().expect("addr")
    }
    fn v4(s: &str) -> SocketAddrV4 {
        s.parse().expect("addr")
    }
    fn ip(s: &str) -> Ipv4Addr {
        s.parse().expect("ip")
    }

    #[test]
    fn what_gets_published() {
        let stable: HashSet<Ipv6Addr> = ["2a02:1810::7".parse().expect("ip")].into();
        let addrs = [
            a("192.168.1.20:443"),
            a("[fd00::20]:443"),
            a("[2a02:1810::7]:443"),
            a("[2a02:1810::beef]:443"), // temporary: left out
        ];
        let r = plan(
            &addrs,
            &stable,
            Some(v4("81.2.69.142:61000")),
            Some(ip("81.2.69.142")),
            443,
        );
        assert_eq!(
            r.published,
            vec![a("[2a02:1810::7]:443"), a("81.2.69.142:61000")]
        );
        assert_eq!(r.port_mapped, Some(a("81.2.69.142:61000")));
        assert_eq!(r.double_nat, Some(false));
    }

    #[test]
    fn no_mapping_publishes_the_outside_address() {
        let r = plan(
            &[a("192.168.1.20:443")],
            &HashSet::new(),
            None,
            Some(ip("81.2.69.142")),
            443,
        );
        assert_eq!(r.published, vec![a("81.2.69.142:443")]);
        assert_eq!(r.double_nat, None);
    }

    #[test]
    fn a_second_nat_publishes_no_ipv4() {
        // The router's own outside address is private (a provider's CGNAT
        // on 10/8, as seen on a real home), or shared, or not what the
        // internet sees.
        for router in [
            "10.130.6.20:61000",
            "100.70.1.2:61000",
            "91.198.174.192:61000",
        ] {
            let r = plan(
                &[a("[2a02:1810::7]:443")],
                &HashSet::new(),
                Some(v4(router)),
                Some(ip("81.2.69.142")),
                443,
            );
            assert_eq!(r.double_nat, Some(true), "{router}");
            assert_eq!(r.port_mapped, None);
            assert_eq!(r.published, vec![a("[2a02:1810::7]:443")], "{router}");
        }
    }

    #[test]
    fn nothing_without_a_way_in() {
        let r = plan(
            &[a("192.168.1.20:443"), a("[fd00::20]:443")],
            &HashSet::new(),
            None,
            None,
            443,
        );
        assert!(r.published.is_empty());
    }
}
