//! connectd running: the link on the box's port, with no ZOOMRIYA server. At
//! home, devices pair and connect over the home network; away, at the box's
//! direct addresses (docs/direct-plus.md). Later phases add the DHT record,
//! the box's own relay behind a Tor onion service, and punching.

use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use tracing::{info, warn};
use zoomriya_keys::SecretKey;

use crate::{
    config::Config,
    devices::Devices,
    link::{Link, LinkConfig, Live},
};

/// Everything connectd shares between its parts.
pub struct Ctx {
    /// Settings.
    pub cfg: Config,
    /// The box key.
    pub box_key: SecretKey,
    /// Paired devices.
    pub devices: Arc<Devices>,
    /// Live connections, while the link runs.
    pub live: Mutex<Option<Live>>,
}

impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ctx")
    }
}

impl Ctx {
    /// Reads the box key and devices from the data directory.
    pub fn open(cfg: Config) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)
            .with_context(|| format!("making {}", cfg.data_dir.display()))?;
        Ok(Self {
            box_key: crate::files::key(&cfg.box_key())?,
            devices: Arc::new(Devices::open(&crate::link::devices_file(&cfg.data_dir))?),
            live: Mutex::new(None),
            cfg,
        })
    }
}

/// Runs the link until `stop` finishes, then closes it.
pub async fn run(ctx: Arc<Ctx>, stop: impl Future<Output = ()>) -> Result<()> {
    let link = Link::start(
        ctx.box_key.clone(),
        LinkConfig {
            bind: vec![
                SocketAddr::from((Ipv4Addr::UNSPECIFIED, ctx.cfg.port)),
                SocketAddr::from((Ipv6Addr::UNSPECIFIED, ctx.cfg.port)),
            ],
            targets: ctx.cfg.targets.clone(),
            pairing_from_loopback: false,
        },
        ctx.devices.clone(),
    )
    .await?;
    if let Ok(mut l) = ctx.live.lock() {
        *l = Some(link.live.clone());
    }
    info!("link on");
    let mdns = announce(&ctx)
        .map_err(|e| warn!("announcing on the home network: {e:#}"))
        .ok();
    stop.await;
    if let Some(m) = mdns {
        let _ = m.shutdown();
    }
    info!("stopping");
    if let Ok(mut l) = ctx.live.lock() {
        *l = None;
    }
    link.router.shutdown().await.ok();
    Ok(())
}

/// Announces the link on the home network, with the box key, so the laptop
/// app finds the box to pair (zoomriya_proto::link::MDNS_SERVICE). Its own
/// host name, so it never clashes with the box's `zoomriya-xxxx.local`.
fn announce(ctx: &Ctx) -> Result<mdns_sd::ServiceDaemon> {
    use zoomriya_proto::link::{MDNS_KEY, MDNS_SERVICE};
    let key = ctx.box_key.public().to_z32();
    let short = &key[..8];
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let info = mdns_sd::ServiceInfo::new(
        MDNS_SERVICE,
        &format!("ZOOMRIYA Box {short}"),
        &format!("zoomriya-link-{short}.local."),
        "",
        ctx.cfg.port,
        &[(MDNS_KEY, key.as_str())][..],
    )?
    .enable_addr_auto();
    mdns.register(info)?;
    info!("announced on the home network");
    Ok(mdns)
}
