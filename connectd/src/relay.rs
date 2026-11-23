//! The box's own iroh relay (docs/direct-plus.md §3): on 127.0.0.1 only,
//! reached from away through the box's onion service. It carries the first
//! packets and the punching until a direct path works, and the data when
//! none does.
//!
//! | Who | Let in |
//! |---|---|
//! | The box itself | ✅ |
//! | A paired device, by its link key | ✅ (iroh checks the key with a signed challenge) |
//! | Anyone else | ❌ |
//!
//! A device removed while connected keeps its relay connection until it
//! drops, but the box refuses its QUIC connections, so it reaches nothing.

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};

use anyhow::{Context, Result};
use iroh::{PublicKey, RelayUrl};
use iroh_relay::server::{Access, AccessControl, ClientRequest, RelayConfig, Server, ServerConfig};
use tracing::debug;

use crate::devices::Devices;

/// Who may use the relay.
#[derive(Debug)]
struct Paired {
    box_key: PublicKey,
    devices: Arc<Devices>,
}

impl Paired {
    fn allows(&self, key: &PublicKey) -> bool {
        *key == self.box_key || self.devices.by_link(key).is_some()
    }
}

impl AccessControl for Paired {
    async fn on_connect(&self, request: &ClientRequest) -> Access {
        let key = request.endpoint_id();
        if self.allows(&key) {
            Access::Allow
        } else {
            debug!(key = %key.fmt_short(), "relay: not a paired device");
            Access::Deny { reason: None }
        }
    }
}

/// The running relay.
#[derive(Debug)]
pub struct Relay {
    server: Server,
    /// Where it listens, for the onion service.
    pub addr: SocketAddr,
    /// Its address for the box's own endpoint.
    pub url: RelayUrl,
}

impl Relay {
    /// Starts the relay on a free port of 127.0.0.1.
    pub async fn start(box_key: PublicKey, devices: Arc<Devices>) -> Result<Self> {
        let mut relay = RelayConfig::new((Ipv4Addr::LOCALHOST, 0));
        relay.access = Arc::new(Paired { box_key, devices });
        let mut cfg = ServerConfig::default();
        cfg.relay = Some(relay);
        let server = Server::spawn(cfg).await.context("starting the relay")?;
        let addr = server.http_addr().context("relay has no address")?;
        let url: RelayUrl = format!("http://{addr}").parse()?;
        Ok(Self { server, addr, url })
    }

    /// Stops it.
    pub async fn stop(self) {
        let _ = self.server.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn only_the_box_and_paired_devices() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let devices = Arc::new(
            Devices::open(&dir.path().join("devices.json")).unwrap_or_else(|e| panic!("{e}")),
        );
        let box_key = SecretKey::generate().public();
        let p = Paired { box_key, devices };
        assert!(p.allows(&box_key));
        assert!(!p.allows(&SecretKey::generate().public()));
    }
}
