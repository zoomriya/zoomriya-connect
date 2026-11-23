//! Finding boxes on the home network: each announces its link over mDNS
//! with its key ([`zoomriya_proto::link::MDNS_SERVICE`]). Anyone on the
//! network could announce anything, so what is found here is only a place to
//! try: pairing proves the box, and after pairing the key is pinned.

use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use zoomriya_keys::PublicKey;
use zoomriya_proto::link::{MDNS_KEY, MDNS_SERVICE};

/// A box seen on the home network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The key it announced.
    pub key: PublicKey,
    /// Where it said its link listens.
    pub addrs: Vec<SocketAddr>,
}

/// Listens for `wait`, and returns every box heard, by key.
pub async fn boxes(wait: Duration) -> anyhow::Result<Vec<Found>> {
    let mdns = ServiceDaemon::new()?;
    let events = mdns.browse(MDNS_SERVICE)?;
    let mut found: BTreeMap<String, Found> = BTreeMap::new();
    let deadline = tokio::time::Instant::now() + wait;
    while let Ok(Ok(event)) = tokio::time::timeout_at(deadline, events.recv_async()).await {
        let ServiceEvent::ServiceResolved(s) = event else {
            continue;
        };
        let Some(key) = s
            .get_property_val_str(MDNS_KEY)
            .and_then(|k| zoomriya_keys::key_from_str(k).ok())
        else {
            continue;
        };
        let entry = found.entry(key.to_z32()).or_insert(Found {
            key,
            addrs: Vec::new(),
        });
        for ip in s.get_addresses() {
            let a = SocketAddr::new(ip.to_ip_addr(), s.get_port());
            if !entry.addrs.contains(&a) {
                entry.addrs.push(a);
            }
        }
    }
    let _ = mdns.shutdown();
    Ok(found.into_values().collect())
}

/// Listens until the box with this key is heard, or `wait` passes: its
/// addresses now, for a box whose addresses changed.
pub async fn the_box(key: &PublicKey, wait: Duration) -> anyhow::Result<Vec<SocketAddr>> {
    Ok(boxes(wait)
        .await?
        .into_iter()
        .find(|f| &f.key == key)
        .map(|f| f.addrs)
        .unwrap_or_default())
}
