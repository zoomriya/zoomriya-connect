//! connectd running: the link on the box's port, with no ZOOMRIYA server. At
//! home, devices pair and connect over the home network; away, at the
//! addresses in the box's DHT record, or through the box's own relay behind
//! its onion service while punching finds a direct path (docs/direct-plus.md).

use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use tracing::{info, warn};
use zoomriya_keys::{SecretKey, record::RecordSecret};

use crate::{
    away::Away,
    config::Config,
    devices::Devices,
    guests::Guests,
    link::{Link, LinkConfig, Live},
    names::Names,
    reach::Reach,
    relay::Relay,
};
use zoomriya_tor::Tor;

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
    /// The secret behind the box's record in the DHT.
    pub record: RecordSecret,
    /// What the box knows about being reached from away.
    pub reach: Mutex<Reach>,
    /// The box's onion address, once its relay runs behind it.
    pub onion: Mutex<Option<String>>,
    /// Whether devices may reach the box away from home.
    pub away: Away,
    /// The family's own name.
    pub names: Arc<Names>,
    /// The guests' door and onion service.
    pub guests: Guests,
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
            record: crate::files::record_secret(&cfg.record_secret())?,
            reach: Mutex::default(),
            onion: Mutex::default(),
            away: Away::open(&cfg.away_file())?,
            names: Arc::new(Names::open(&cfg.names_file())?),
            guests: Guests::default(),
            cfg,
        })
    }
}

/// Runs the link until `stop` finishes, then closes it.
pub async fn run(ctx: Arc<Ctx>, stop: impl Future<Output = ()>) -> Result<()> {
    let (relay, tor) = if ctx.cfg.tor {
        let relay =
            Relay::start(ctx.box_key.public(), ctx.devices.clone(), ctx.away.clone()).await?;
        let tor = start_tor(&ctx).map_err(|e| warn!("Tor: {e:#}")).ok();
        (Some(relay), tor)
    } else {
        (None, None)
    };
    let serve = |name: &str, to: SocketAddr| {
        tor.as_ref().and_then(|t| {
            t.serve(name, to)
                .map_err(|e| warn!("the {name} onion service: {e:#}"))
                .ok()
        })
    };
    let onion = relay.as_ref().and_then(|r| serve("box-relay", r.addr));
    let onion_address = onion.as_ref().map(|o| o.address.clone());
    if let Ok(mut o) = ctx.onion.lock() {
        o.clone_from(&onion_address);
    }
    let link = Link::start(
        ctx.box_key.clone(),
        LinkConfig {
            bind: vec![
                SocketAddr::from((Ipv4Addr::UNSPECIFIED, ctx.cfg.port)),
                SocketAddr::from((Ipv6Addr::UNSPECIFIED, ctx.cfg.port)),
            ],
            targets: ctx.cfg.targets.clone(),
            pairing_from_loopback: false,
            record: Some(ctx.record.clone()),
            relay: relay.as_ref().map(|r| r.url.clone()),
            onion: onion_address,
            away: ctx.away.clone(),
        },
        ctx.devices.clone(),
    )
    .await?;
    if let Ok(mut l) = ctx.live.lock() {
        *l = Some(link.live.clone());
    }
    info!("link on");
    let publishing = tokio::spawn(crate::reach::keep_published(
        ctx.clone(),
        link.router.endpoint().clone(),
        ctx.record.clone(),
    ));
    let naming = tokio::spawn(crate::names::keep(ctx.clone()));
    let guests = tokio::spawn(crate::guests::keep(ctx.clone(), tor.clone()));
    let mdns = announce(&ctx)
        .map_err(|e| warn!("announcing on the home network: {e:#}"))
        .ok();
    stop.await;
    publishing.abort();
    naming.abort();
    guests.abort();
    if let Some(m) = mdns {
        let _ = m.shutdown();
    }
    info!("stopping");
    if let Ok(mut l) = ctx.live.lock() {
        *l = None;
    }
    link.router.shutdown().await.ok();
    drop(onion);
    if let Some(r) = relay {
        r.stop().await;
    }
    Ok(())
}

/// Tor, and the onion service in front of the relay. Tor joins the network
/// in the background; the address is known at once.
fn start_tor(ctx: &Ctx) -> Result<Tor> {
    for dir in [ctx.cfg.tor_state(), ctx.cfg.tor_cache()] {
        std::fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        restrict(&dir)?;
    }
    Tor::start(&ctx.cfg.tor_state(), &ctx.cfg.tor_cache(), None)
}

/// Tor refuses folders others can read or write.
fn restrict(dir: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restricting {}", dir.display()))
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
