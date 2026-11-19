//! Telling relays which devices the box removed: at once when an admin
//! removes one, and the whole list again whenever the box reconnects
//! (docs/architecture.md, "Removing a device").

use std::time::Duration;

use tracing::{info, warn};
use zoomriya_keys::{Purpose, SecretKey, seal};
use zoomriya_proto::{RemovedDevice, RemovedDevices};

/// Sends the list to every relay. Returns how many took it.
pub async fn send(
    http: &reqwest::Client,
    relays: &[String],
    box_key: &SecretKey,
    issued: u64,
    full: bool,
    devices: Vec<RemovedDevice>,
) -> usize {
    let list = RemovedDevices {
        box_key: box_key.public().into(),
        issued,
        full,
        devices,
    };
    let body = seal(box_key, Purpose::RemovedDevices, &list);
    let mut took = 0;
    for relay in relays {
        let url = format!("{}/zoomriya/v1/removed", relay.trim_end_matches('/'));
        match http
            .post(&url)
            .timeout(Duration::from_secs(15))
            .body(body.clone())
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => took += 1,
            // An older list than one it already has: fine.
            Ok(r) if r.status() == reqwest::StatusCode::CONFLICT => took += 1,
            Ok(r) => warn!(%relay, status = %r.status(), "relay didn't take the removed list"),
            Err(e) => warn!(%relay, "relay unreachable for the removed list: {e}"),
        }
    }
    if took > 0 {
        info!(
            relays = took,
            devices = list.devices.len(),
            full,
            "removed list sent"
        );
    }
    took
}
