//! connectd running: registered with its server, on the relay, keeping its
//! name pointed at home and its certificate fresh, and telling relays about
//! removed devices. Box Desktop changes things through the API; each change
//! restarts the online part.

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use iroh::{RelayUrl, Watcher};
use serde::Serialize;
use tokio::sync::Notify;
use tracing::{info, warn};
use zoomriya_keys::SecretKey;
use zoomriya_proto::{HomeAddrs, RegisterRequest, RelayUse, WellKnown};

use crate::{
    acme,
    config::Config,
    devices::Devices,
    laptop,
    link::{Link, LinkConfig, Live},
    net, relays,
    server::{self, Server},
    state::Saved,
};

/// What Box Desktop shows about ZOOMRIYA Connect.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Health {
    /// The relay we're connected to, if any.
    pub relay: Option<String>,
    /// Whether the relay connection is up.
    pub relay_connected: bool,
    /// The relay turned us away, and why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_refused: Option<String>,
    /// The addresses the name points at.
    pub home: HomeAddrs,
    /// Days left on the certificate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_days: Option<i64>,
    /// Who issued it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_authority: Option<String>,
    /// The families' account page on the server.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub account_page: String,
    /// This month's use of the server's relays, and the allowance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_use: Option<RelayUse>,
    /// Problems, by kind, in plain words.
    pub problems: BTreeMap<&'static str, String>,
}

/// Everything connectd shares between its parts.
pub struct Ctx {
    /// Settings.
    pub cfg: Config,
    /// The box key.
    pub box_key: SecretKey,
    /// The DNS update key.
    pub dns_key: SecretKey,
    /// Saved state.
    pub state: Saved,
    /// Paired devices.
    pub devices: Arc<Devices>,
    /// What Box Desktop shows.
    pub health: Mutex<Health>,
    /// Live connections, while the link runs.
    pub live: Mutex<Option<Live>>,
    /// The relays, from the server.
    pub relays: Mutex<Vec<String>>,
    /// Set when something changed that needs the online part restarted.
    pub changed: Notify,
    /// For relays and the server.
    pub http: reqwest::Client,
}

impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ctx")
    }
}

impl Ctx {
    /// Reads keys, state and devices from the data directory.
    pub fn open(cfg: Config) -> Result<Self> {
        if cfg.trust_clock {
            crate::clock::trust_clock();
        }
        std::fs::create_dir_all(&cfg.data_dir)
            .with_context(|| format!("making {}", cfg.data_dir.display()))?;
        Ok(Self {
            box_key: crate::files::key(&cfg.box_key())?,
            dns_key: crate::files::key(&cfg.dns_key())?,
            state: Saved::open(&cfg.data_dir.join("state.json"))?,
            devices: Arc::new(Devices::open(&crate::link::devices_file(&cfg.data_dir))?),
            health: Mutex::default(),
            live: Mutex::new(None),
            relays: Mutex::default(),
            changed: Notify::new(),
            http: reqwest::Client::builder()
                .user_agent(concat!("zoomriya-connectd/", env!("CARGO_PKG_VERSION")))
                .build()?,
            cfg,
        })
    }

    /// The server's address in use.
    pub fn server_url(&self) -> String {
        self.state
            .get()
            .server
            .unwrap_or_else(|| self.cfg.server.clone())
    }

    /// Changes the health.
    pub fn health<T>(&self, f: impl FnOnce(&mut Health) -> T) -> Option<T> {
        self.health.lock().ok().map(|mut h| f(&mut h))
    }

    fn problem(&self, kind: &'static str, words: Option<String>) {
        self.health(|h| match words {
            Some(w) => {
                h.problems.insert(kind, w);
            }
            None => {
                h.problems.remove(kind);
            }
        });
    }

    /// The next "issued" time for a removed-device list: always later than the last.
    pub fn next_issued(&self) -> u64 {
        let now = crate::now();
        self.state
            .update(|s| {
                s.removed_issued = now.max(s.removed_issued + 1);
                s.removed_issued
            })
            .unwrap_or(now)
    }

    /// Sends relays the removed-device list.
    pub async fn send_removed(&self, full: bool, only: Option<zoomriya_proto::RemovedDevice>) {
        let relays = self.relays.lock().map(|r| r.clone()).unwrap_or_default();
        if relays.is_empty() {
            return;
        }
        let devices = match only {
            Some(d) if !full => vec![d],
            _ => self.devices.removed_list(crate::now()),
        };
        let issued = self.next_issued();
        relays::send(&self.http, &relays, &self.box_key, issued, full, devices).await;
    }

    /// The registration request for `name`.
    pub fn register_request(&self, name: &str) -> RegisterRequest {
        RegisterRequest {
            name: name.into(),
            box_key: self.box_key.public().into(),
            dns_update_key: self.dns_key.public().into(),
            home: net::home_addrs(),
            factory_token: std::fs::read_to_string(&self.cfg.factory_token_file)
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
            account: None,
        }
    }
}

/// Runs forever: the online part while registered and switched on.
pub async fn run(ctx: Arc<Ctx>) {
    loop {
        let st = ctx.state.get();
        if st.off || st.name.is_none() {
            ctx.health(|h| *h = Health::default());
            ctx.changed.notified().await;
            continue;
        }
        if let Err(e) = online(&ctx).await {
            warn!("ZOOMRIYA Connect: {e:#}");
            // The account's own problems get their own kind, so Box Desktop
            // can ask for it again.
            let kind = match server::said(&e).map(|s| s.code.as_str()) {
                Some("account_needed" | "account_blocked") => "account",
                _ => "server",
            };
            ctx.problem(kind, Some(problem_words(&e)));
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                _ = ctx.changed.notified() => {}
            }
        }
    }
}

fn problem_words(e: &anyhow::Error) -> String {
    match server::said(e) {
        Some(s) => s.message.clone(),
        None => "The box can't reach the ZOOMRIYA Connect server. It keeps trying.".into(),
    }
}

fn well_known_cache(ctx: &Ctx) -> PathBuf {
    ctx.cfg.data_dir.join("well-known.json")
}

async fn well_known(ctx: &Ctx, server: &Server) -> Result<WellKnown> {
    match server.well_known().await {
        Ok(wk) => {
            let _ = crate::files::write_private(&well_known_cache(ctx), &serde_json::to_vec(&wk)?);
            Ok(wk)
        }
        Err(e) => {
            // The server is down: keep going with what it said last time.
            let bytes = std::fs::read(well_known_cache(ctx)).map_err(|_| e)?;
            Ok(serde_json::from_slice(&bytes)?)
        }
    }
}

async fn online(ctx: &Arc<Ctx>) -> Result<()> {
    let st = ctx.state.get();
    let name = st.name.clone().context("no name")?;
    let server = Server::new(&ctx.server_url())?;
    let wk = well_known(ctx, &server).await?;
    ctx.health(|h| h.account_page = wk.account_page.clone());

    // Register again each time: it takes the current DNS update key and home
    // addresses, and puts back a registration a server restore lost.
    match server
        .register(&ctx.box_key, &ctx.register_request(&name))
        .await
    {
        Ok(r) => {
            ctx.state.update(|s| {
                s.fqdn = Some(r.fqdn.clone());
                s.share_fqdn = Some(r.share_fqdn.clone());
                s.account = r.account.clone();
            })?;
            ctx.problem("server", None);
            ctx.problem("account", None);
        }
        // Unreachable: carry on with the name we have.
        Err(e) if server::said(&e).is_none() => warn!("registering: {e:#}"),
        Err(e) => return Err(e),
    }
    let st = ctx.state.get();
    let fqdn = st.fqdn.clone().context("no full name yet")?;

    let relays: Vec<String> = if st.relay_off {
        vec![]
    } else {
        wk.relays.clone()
    };
    if let Ok(mut r) = ctx.relays.lock() {
        *r = relays.clone();
    }
    let relay_urls: Vec<RelayUrl> = relays.iter().filter_map(|u| u.parse().ok()).collect();
    let link = Link::start(
        ctx.box_key.clone(),
        LinkConfig {
            relays: relay_urls,
            address_records: wk.address_records.parse().ok(),
            bind: vec![
                SocketAddr::from((Ipv4Addr::UNSPECIFIED, ctx.cfg.port)),
                SocketAddr::from((Ipv6Addr::UNSPECIFIED, ctx.cfg.port)),
            ],
            targets: ctx.cfg.targets.clone(),
            fqdn: fqdn.clone(),
            server: server.base().to_string(),
            pairing_from_loopback: false,
            extra_roots: ctx.cfg.extra_roots()?,
        },
        ctx.devices.clone(),
    )
    .await?;
    if let Ok(mut l) = ctx.live.lock() {
        *l = Some(link.live.clone());
    }
    info!(%fqdn, "ZOOMRIYA Connect on");

    let mut tasks = tokio::task::JoinSet::new();
    // Laptops: WebRTC, and their pairing and setup at home.
    let lc = &ctx.cfg.laptops;
    match ctx.cfg.targets.iter().find(|t| t.name == lc.target) {
        Some(target) => {
            let hosts = net::home_addrs()
                .ipv4
                .into_iter()
                .map(std::net::IpAddr::V4)
                .collect();
            match laptop::Laptops::start(
                ctx.devices.clone(),
                ctx.box_key.clone(),
                laptop::Setup {
                    fqdn: fqdn.clone(),
                    server: server.base().to_string(),
                    target: target.clone(),
                    udp_port: lc.udp_port,
                    hosts,
                    relays: relays.clone(),
                    stun: wk.stun.clone(),
                    desktop: if lc.desktop_url.is_empty() {
                        format!("https://{fqdn}:7443")
                    } else {
                        lc.desktop_url.trim_end_matches('/').to_string()
                    },
                    bundle: ctx.cfg.certs_dir.join(acme::BUNDLE),
                },
            )
            .await
            {
                Ok((laptops, run)) => {
                    tasks.spawn(run);
                    tasks.spawn(laptop::listen_on_relays(laptops.clone(), ctx.http.clone()));
                    tasks.spawn(laptop::serve_home(
                        laptops,
                        lc.tcp_port,
                        ctx.cfg.certs_dir.join(acme::BUNDLE),
                        lc.plain_http,
                    ));
                }
                Err(e) => warn!("laptops: {e:#}"),
            }
        }
        None => warn!(target = %lc.target, "laptops: no such target"),
    }
    tasks.spawn(keep_home(ctx.clone(), server.clone(), name.clone()));
    tasks.spawn(keep_certificate(ctx.clone(), server.clone(), name.clone()));
    tasks.spawn(watch_relay(
        ctx.clone(),
        link.endpoint().clone(),
        server.clone(),
        name.clone(),
    ));
    if !st.relay_off {
        tasks.spawn(keep_relay_use(ctx.clone(), server.clone(), name.clone()));
    }

    ctx.changed.notified().await;
    tasks.abort_all();
    if let Ok(mut l) = ctx.live.lock() {
        *l = None;
    }
    link.router.shutdown().await.ok();
    info!("ZOOMRIYA Connect restarting with the new settings");
    Ok(())
}

/// Keeps the name pointed at the box's home addresses.
async fn keep_home(ctx: Arc<Ctx>, server: Server, name: String) {
    let mut last: Option<HomeAddrs> = None;
    loop {
        let home = net::home_addrs();
        if last.as_ref() != Some(&home) {
            match server.set_home(&ctx.dns_key, &name, &home).await {
                Ok(()) => {
                    ctx.health(|h| h.home = home.clone());
                    ctx.problem("home", None);
                    last = Some(home);
                }
                Err(e) => {
                    warn!("pointing the name home: {e:#}");
                    if server::said(&e).is_some_and(|s| s.code == "not_your_box") {
                        // The registration went missing: register again.
                        let _ = server
                            .register(&ctx.box_key, &ctx.register_request(&name))
                            .await;
                    }
                    ctx.problem("home", Some(problem_words(&e)));
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(300)).await;
    }
}

/// Keeps the certificate fresh: checks twice a day, renews a month ahead.
async fn keep_certificate(ctx: Arc<Ctx>, server: Server, name: String) {
    let bundle = ctx.cfg.certs_dir.join(acme::BUNDLE);
    loop {
        let st = ctx.state.get();
        let days = acme::days_left(&bundle, crate::now());
        ctx.health(|h| h.certificate_days = days);
        let due = days.is_none_or(|d| d < acme::RENEW_DAYS);
        let mut wait = Duration::from_secs(12 * 3600);
        if due && let (Some(fqdn), Some(share)) = (&st.fqdn, &st.share_fqdn) {
            match acme::obtain(
                &server,
                &ctx.dns_key,
                &name,
                fqdn,
                share,
                &ctx.cfg.certificate_authorities,
                &ctx.cfg.data_dir,
                &ctx.cfg.certs_dir,
            )
            .await
            {
                Ok(who) => {
                    let days = acme::days_left(&bundle, crate::now());
                    ctx.health(|h| {
                        h.certificate_days = days;
                        h.certificate_authority = Some(who);
                    });
                    ctx.problem("certificate", None);
                }
                Err(e) => {
                    warn!("certificate: {e:#}");
                    ctx.problem(
                        "certificate",
                        Some(
                            "The box couldn't get its certificate yet. It tries again in an hour."
                                .into(),
                        ),
                    );
                    wait = Duration::from_secs(3600);
                }
            }
        }
        tokio::time::sleep(wait).await;
    }
}

/// Asks how much of the month's relay allowance the box has used: soon after
/// starting, then every hour. Nothing to show if the server doesn't say.
async fn keep_relay_use(ctx: Arc<Ctx>, server: Server, name: String) {
    tokio::time::sleep(Duration::from_secs(30)).await;
    loop {
        match server.relay_use(&ctx.box_key, &name).await {
            Ok(u) => {
                ctx.health(|h| h.relay_use = Some(u));
            }
            Err(e) => tracing::debug!("relay use: {e:#}"),
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

/// Watches the relay connection: each time it comes up, the relay gets the
/// whole removed list again; and once an hour anyway.
async fn watch_relay(ctx: Arc<Ctx>, endpoint: iroh::Endpoint, server: Server, name: String) {
    let mut status = endpoint.home_relay_status();
    let mut was_up = false;
    let mut was_refused = false;
    let mut last_full = 0u64;
    loop {
        let list = status.get();
        let up = list.iter().any(|s| s.is_connected());
        let url = list.first().map(|s| s.url().to_string());
        let refused = list
            .iter()
            .find_map(|s| s.auth_denied_reason().map(String::from));
        let refused_now = refused.is_some();
        ctx.health(|h| {
            h.relay = url;
            h.relay_connected = up;
            h.relay_refused = refused;
        });
        // Turned away: ask the server why, so Box Desktop can say it plainly
        // (a blocked account, most likely), and clear it once it's let in.
        if refused_now && !was_refused {
            match server
                .register(&ctx.box_key, &ctx.register_request(&name))
                .await
            {
                Err(e) if server::said(&e).is_some_and(|s| s.code.starts_with("account_")) => {
                    ctx.problem("account", Some(problem_words(&e)));
                }
                _ => {}
            }
        }
        if up {
            ctx.problem("account", None);
        }
        was_refused = refused_now;
        let now = crate::now();
        if up && (!was_up || now >= last_full + 3600) {
            ctx.send_removed(true, None).await;
            last_full = now;
        }
        was_up = up;
        tokio::select! {
            _ = status.updated() => {}
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
        }
    }
}
