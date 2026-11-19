//! `zoomriya-control-plane`: registrations, box names in DNS, address records,
//! and the list of boxes for relays. It never carries family data and never
//! learns a family's devices.
//!
//! ```text
//! zoomriya-control-plane run          --config FILE
//! zoomriya-control-plane status       --config FILE
//! zoomriya-control-plane factory-tokens --config FILE TOKENS_FILE
//! zoomriya-control-plane switch-off   --config FILE NAME
//! zoomriya-control-plane switch-on    --config FILE NAME
//! zoomriya-control-plane mark-stolen  --config FILE NAME
//! zoomriya-control-plane reports      --config FILE
//! zoomriya-control-plane report-done  --config FILE ID
//! zoomriya-control-plane admin-token  --config FILE NAME
//! zoomriya-control-plane secondary    --config NS2_FILE
//! ```
//!
//! The commands work on the database directly; a running server notices
//! within a second.

use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use tracing::{info, warn};
use zoomriya_proto::{PROTOCOL_VERSION, WellKnown, names::NameRules};

mod admin_api;
mod api;
mod check;
mod config;
mod dns;
mod limit;
mod notes;
mod signal;
mod store;
mod turn;
mod zone;

#[cfg(test)]
mod e2e;

use config::Config;
use store::{Rules, Store};

const USAGE: &str = "usage: zoomriya-control-plane COMMAND --config FILE [ARG]
commands: run, status, factory-tokens FILE, switch-off NAME, switch-on NAME,
          mark-stolen NAME, reports, report-done ID, admin-token NAME,
          secondary (on the second DNS server, with its own config)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, config, arg) = match args.as_slice() {
        [c, flag, path] if flag == "--config" => (c.as_str(), PathBuf::from(path), None),
        [c, flag, path, a] if flag == "--config" => {
            (c.as_str(), PathBuf::from(path), Some(a.as_str()))
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command_main(command, config, arg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("zoomriya-control-plane: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn command_main(command: &str, path: PathBuf, arg: Option<&str>) -> Result<()> {
    if command == "secondary" {
        let cfg = config::SecondaryConfig::load(&path)?;
        zoomriya_serve::init_logging();
        let _ = rustls::crypto::ring::default_provider().install_default();
        return tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(secondary(cfg));
    }
    let cfg = Config::load(&path)?;
    let needs = |what: &str| arg.with_context(|| format!("{command} needs {what}\n{USAGE}"));
    match command {
        "run" => {
            zoomriya_serve::init_logging();
            let _ = rustls::crypto::ring::default_provider().install_default();
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(run(cfg))
        }
        "status" => {
            let (all, factory, stolen, off) = Store::open(&cfg.database)?.counts()?;
            println!("Boxes registered: {all}");
            println!("  with a factory token: {factory}");
            println!("  marked stolen:        {stolen}");
            println!("  switched off:         {off}");
            Ok(())
        }
        "factory-tokens" => {
            let file = needs("a file of tokens")?;
            let text = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
            let n = Store::open(&cfg.database)?.import_factory_tokens(text.lines())?;
            println!(
                "{n} new factory tokens. Only their hashes are kept: store the file safely or delete it."
            );
            Ok(())
        }
        "switch-off" | "switch-on" => {
            let name = needs("a box name")?;
            let off = command == "switch-off";
            if !Store::open(&cfg.database)?.set_disabled(name, off)? {
                bail!("no box is called {name}");
            }
            println!(
                "{name}.{} {}",
                cfg.names.zone,
                if off {
                    "no longer answers."
                } else {
                    "answers again."
                }
            );
            Ok(())
        }
        "mark-stolen" => {
            let name = needs("a box name")?;
            if !Store::open(&cfg.database)?.set_stolen(name)? {
                bail!("no box is called {name}");
            }
            println!("{name} is marked stolen. Relays refuse it and its devices within seconds.");
            Ok(())
        }
        "reports" => {
            let reports = Store::open(&cfg.database)?.open_reports()?;
            if reports.is_empty() {
                println!("No open reports.");
            }
            for (id, name, reason, at) in reports {
                println!("#{id}  {name}  (at {at})\n    {reason}");
            }
            Ok(())
        }
        "report-done" => {
            let id: i64 = needs("a report number")?
                .trim_start_matches('#')
                .parse()
                .context("a report number")?;
            if !Store::open(&cfg.database)?.close_report(id)? {
                bail!("no report #{id}");
            }
            println!("Report #{id} closed.");
            Ok(())
        }
        "admin-token" => {
            let name = needs("a name for whoever will use it")?;
            let admin = cfg
                .admin
                .as_ref()
                .context("there's no [admin] section in the config")?;
            let token = zoomriya_serve::admin::new_token(&admin.tokens_file, name)?;
            println!("Sign in to the GUI with this token. It's shown only now:\n\n    {token}\n");
            println!(
                "To take it back: remove the line starting \"{name} \" from {}.",
                admin.tokens_file.display()
            );
            Ok(())
        }
        _ => bail!("{USAGE}"),
    }
}

/// Everything the server needs, apart from listening.
pub fn build(cfg: &Config, store: Arc<Store>) -> Result<(Arc<api::Api>, dns::DnsHandler)> {
    let words = cfg.blocked_words()?;
    let rules = Rules {
        names: NameRules::new().with_extra_words(words.lines()),
        per_address_per_day: cfg.registration.per_address_per_day,
        released_name_days: cfg.registration.released_name_days,
    };
    let limits = api::Limits::default();
    let turn = match &cfg.guests {
        Some(g) => Some(turn::Turn {
            urls: g.turn_urls.clone(),
            secrets: config::read_keys(&g.turn_secrets_file)?,
        }),
        None => None,
    };
    let zone_keys = match &cfg.zone_copy {
        Some(z) => config::read_keys(&z.keys_file)?,
        None => vec![],
    };
    let base = cfg
        .names
        .control_plane_url
        .trim_end_matches('/')
        .to_string();
    let api = Arc::new(api::Api {
        store: store.clone(),
        rules,
        relay_keys: cfg.relay_keys()?,
        well_known: WellKnown {
            version: PROTOCOL_VERSION,
            address_records: format!("{base}/pkarr"),
            control_plane: base,
            relays: cfg.relays.urls.clone(),
            share: cfg.names.share_url.clone(),
            zone: cfg.names.zone.clone(),
        },
        zone: cfg.names.zone.clone(),
        clock_allowance: cfg.registration.clock_allowance_seconds,
        now: zoomriya_serve::now,
        per_addr: limits.per_addr,
        per_record: limits.per_record,
        per_report: limits.per_report,
        per_box: limits.per_box,
        per_box_turn: limits.per_box_turn,
        per_guest: limits.per_guest,
        turn,
        checker: check::DoorChecker::new(),
        hub: signal::Hub::default(),
        share_origin: cfg.names.share_url.trim_end_matches('/').to_string(),
        zone_keys,
    });
    let dns = dns::DnsHandler::new(store, zones(&cfg.names), cfg.dns.answers_per_second);
    Ok((api, dns))
}

fn zones(n: &config::Names) -> dns::Zones {
    dns::Zones {
        boxes: n.zone.trim_end_matches('.').to_ascii_lowercase(),
        records: n.address_zone.trim_end_matches('.').to_ascii_lowercase(),
        nameservers: n.nameservers.clone(),
        hostmaster: n.hostmaster.clone(),
        certificate_authorities: n.certificate_authorities.clone(),
    }
}

/// The second DNS server: DNS only, from a copy of the first's zone.
async fn secondary(cfg: config::SecondaryConfig) -> Result<()> {
    let key = std::fs::read_to_string(&cfg.key_file)
        .with_context(|| format!("reading {}", cfg.key_file.display()))?
        .trim()
        .to_string();
    anyhow::ensure!(!key.is_empty(), "{} is empty", cfg.key_file.display());
    // Nothing is kept on disk: the copy lives in memory.
    let store = Arc::new(Store::open_memory()?);
    let handler =
        dns::DnsHandler::new(store.clone(), zones(&cfg.names), cfg.dns.answers_per_second);
    dns::serve(cfg.dns.bind, handler).await?;
    tokio::spawn(zone::follow(
        store,
        cfg.primary_url.clone(),
        key,
        Duration::from_secs(cfg.poll_seconds.max(1)),
    ));
    info!("second DNS server running, copying {}", cfg.primary_url);
    zoomriya_serve::shutdown_signal().await;
    info!("stopping");
    Ok(())
}

async fn run(cfg: Config) -> Result<()> {
    let store = Arc::new(Store::open(&cfg.database)?);
    let (api, dns) = build(&cfg, store.clone())?;
    dns::serve(cfg.dns.bind, dns).await?;

    // Notice the operator commands' changes.
    let watched = store.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            match watched.reload_if_changed() {
                Ok(true) => info!("database changed outside, reloaded"),
                Ok(false) => {}
                Err(err) => warn!("reloading: {err:#}"),
            }
        }
    });

    if let Some(admin) = cfg.admin.clone() {
        let relays = Arc::new(std::sync::Mutex::new(Vec::new()));
        tokio::spawn(admin_api::watch_relays(
            cfg.relays.urls.clone(),
            relays.clone(),
        ));
        let routes = admin_api::router(Arc::new(admin_api::Admin {
            api: api.clone(),
            relays,
        }));
        let router = zoomriya_serve::admin::router(
            routes,
            zoomriya_serve::admin::Tokens::new(admin.tokens_file.clone()),
            &admin.web_dir,
        );
        tokio::spawn(async move {
            if let Err(err) = zoomriya_serve::admin::serve(&admin, router).await {
                tracing::error!("GUI: {err:#}");
            }
        });
    }

    let listener =
        zoomriya_serve::TlsListener::bind(cfg.https.bind, &cfg.https.cert, &[b"http/1.1"]).await?;
    let router = api::router(api);
    let (all, ..) = store.counts()?;
    info!(boxes = all, "control plane running");
    tokio::select! {
        res = zoomriya_serve::serve_https(listener, router) => res,
        _ = zoomriya_serve::shutdown_signal() => {
            info!("stopping");
            Ok(())
        }
    }
}
