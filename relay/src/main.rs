//! `zoomriya-relay`: ZOOMRIYA Connect's relay, built on iroh's relay library
//! with our own check for passes and removed devices.
//!
//! ```text
//! zoomriya-relay run --config /etc/zoomriya-relay/relay.toml
//! zoomriya-relay enrol-code --config /etc/zoomriya-relay/relay.toml
//! ```

use std::{num::NonZeroU32, path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use http::HeaderMap;
use iroh_metrics::{Registry, service::MetricsServer};
use iroh_relay::{
    KeyCache,
    server::{
        ClientRateLimit, Metrics, QuicConfig, Server, ServerConfig,
        http_server::{Handlers, RelayService},
    },
};
use tokio::sync::Mutex;
use tracing::{error, info};

mod admin_api;
mod boxes;
mod config;
mod enrol;
mod front;
mod gate;
mod laptops;
mod meter;
mod sni;
mod turn;

use config::{Boxes, Config};
use gate::{Gate, GateAccess, GateConfig};

const USAGE: &str = "usage: zoomriya-relay (run | enrol-code) --config FILE
       zoomriya-relay admin-token --config FILE NAME";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let config = match (args.next().as_deref(), args.next()) {
        (Some("--config"), Some(path)) => PathBuf::from(path),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let arg = args.next();
    let result = match command.as_deref() {
        Some("run") => run(config),
        Some("enrol-code") => enrol_code(config),
        Some("admin-token") => admin_token(config, arg),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("zoomriya-relay: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn enrol_code(path: PathBuf) -> Result<()> {
    let cfg = Config::load(&path)?;
    anyhow::ensure!(
        matches!(cfg.boxes, Boxes::Enrolled),
        "this relay gets its boxes from its control plane, so it has no codes"
    );
    let code = enrol::new_code(&cfg.state_dir, zoomriya_serve::now())?;
    println!("Type this code into Box Desktop, under ZOOMRIYA Connect:");
    println!();
    println!("    {code}");
    println!();
    println!("It works once, for the next 10 minutes.");
    Ok(())
}

fn admin_token(path: PathBuf, name: Option<String>) -> Result<()> {
    let cfg = Config::load(&path)?;
    let name = name.context("admin-token needs a name for whoever will use it")?;
    let admin = cfg
        .admin
        .as_ref()
        .context("there's no [admin] section in the config")?;
    let token = zoomriya_serve::admin::new_token(&admin.tokens_file, &name)?;
    println!("Sign in to the GUI with this token. It's shown only now:\n\n    {token}\n");
    println!(
        "To take it back: remove the line starting \"{name} \" from {}.",
        admin.tokens_file.display()
    );
    Ok(())
}

fn run(path: PathBuf) -> Result<()> {
    zoomriya_serve::init_logging();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cfg = Config::load(&path)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(cfg))
}

async fn serve(cfg: Config) -> Result<()> {
    std::fs::create_dir_all(&cfg.state_dir)
        .with_context(|| format!("making {}", cfg.state_dir.display()))?;

    let gate = Arc::new(Gate::new(GateConfig {
        clock_allowance: cfg.limits.clock_allowance_seconds,
        longest_pass: cfg.limits.longest_pass_days * 86_400,
    }));
    let boxes_file = boxes::path(&cfg.state_dir);
    let seq = boxes::restore(&boxes_file, &gate)?;

    // iroh's relay service, with our gate as its access check.
    let relay_metrics = Arc::new(Metrics::default());
    let rate_limit = cfg
        .limits
        .client_bytes_per_second
        .and_then(NonZeroU32::new)
        .map(|bps| {
            let mut limit = ClientRateLimit::new(bps);
            limit.max_burst_bytes = bps.checked_mul(NonZeroU32::MIN.saturating_add(1));
            limit
        });
    let relay = RelayService::new(
        Handlers::default(),
        HeaderMap::new(),
        rate_limit,
        KeyCache::new(1024 * 1024),
        Arc::new(GateAccess(gate.clone())),
        relay_metrics.clone(),
    );
    gate.set_clients(relay.clients().clone());

    let mut tasks = tokio::task::JoinSet::new();
    let started = zoomriya_serve::now();

    // The TURN server beside this relay: counted from its log, checked
    // every half minute.
    let turn = match &cfg.turn {
        Some(t) => {
            let reports = matches!(cfg.boxes, Boxes::ControlPlane { .. });
            let turn = Arc::new(turn::Turn::new(t.clone(), reports)?);
            let follow = turn.clone();
            tasks.spawn(async move { follow.follow_log().await });
            let check = turn.clone();
            tasks.spawn(async move {
                let mut every = tokio::time::interval(Duration::from_secs(30));
                loop {
                    every.tick().await;
                    check.check().await;
                }
            });
            Some(turn)
        }
        None => None,
    };

    // Where the list of boxes comes from.
    let enrolled = match &cfg.boxes {
        Boxes::ControlPlane {
            url,
            key_file,
            poll_seconds,
            name,
        } => {
            let key = std::fs::read_to_string(key_file)
                .with_context(|| format!("reading {}", key_file.display()))?
                .trim()
                .to_string();
            anyhow::ensure!(!key.is_empty(), "{} is empty", key_file.display());
            tasks.spawn(boxes::follow(
                gate.clone(),
                url.clone(),
                key,
                name.clone(),
                Duration::from_secs((*poll_seconds).max(1)),
                boxes_file.clone(),
                seq,
                boxes::Side {
                    turn: turn.clone(),
                    state_dir: cfg.state_dir.clone(),
                    started,
                },
            ));
            None
        }
        Boxes::Enrolled => Some(front::Enrolled {
            state_dir: cfg.state_dir.clone(),
            lock: Arc::new(Mutex::new(())),
        }),
    };

    // HTTPS: the relay and our API.
    // TURN over TLS may ask for its own protocol name.
    let alpn: &[&[u8]] = if turn.is_some() {
        &[b"http/1.1", b"stun.turn"]
    } else {
        &[b"http/1.1"]
    };
    let listener = zoomriya_serve::TlsListener::bind(cfg.https.bind, &cfg.https.cert, alpn).await?;
    let tls = listener.config();
    let laptops = Arc::new(laptops::Hub::default());
    {
        let laptops = laptops.clone();
        tasks.spawn(async move {
            let mut every = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                every.tick().await;
                laptops.tidy();
            }
        });
    }
    let api = front::router(front::Api {
        gate: gate.clone(),
        enrolled,
        clock_allowance: cfg.limits.clock_allowance_seconds,
        laptops: laptops.clone(),
        turn: turn.clone(),
    });
    let routes = front::Routes {
        pass_through: cfg
            .pass_through
            .iter()
            .map(|p| (p.name.to_ascii_lowercase(), p.to))
            .collect(),
        turn: turn.as_ref().map(|t| t.route()),
    };
    tasks.spawn(front::serve(
        listener,
        relay.clone(),
        api,
        gate.meter.clone(),
        routes,
    ));

    // Plain HTTP: the captive-portal check only.
    if let Some(http) = &cfg.http {
        let l = tokio::net::TcpListener::bind(http.bind)
            .await
            .with_context(|| format!("listening on {}", http.bind))?;
        info!("captive-portal check on {}", http.bind);
        tasks.spawn(async move {
            if let Err(err) = zoomriya_serve::serve_plain(l, front::captive_portal()).await {
                error!("captive-portal check: {err:#}");
            }
        });
    }

    // QUIC address discovery, from iroh's own server with only QUIC on.
    let mut quic_server = None;
    if let Some(quic) = &cfg.quic {
        let mut qc = QuicConfig::new(quic.bind);
        let mut server_config = (*tls).clone();
        server_config.alpn_protocols = vec![];
        qc.server_config = Some(server_config);
        let mut sc = ServerConfig::default();
        sc.quic = Some(qc);
        let server = Server::spawn(sc).await.context("QUIC address discovery")?;
        info!("QUIC address discovery on {}", quic.bind);
        quic_server = Some(server);
    }

    // Numbers, for our dashboard only.
    let _metrics = if let Some(m) = &cfg.metrics {
        let mut registry = Registry::default();
        registry.register(relay_metrics);
        registry.register(gate.metrics.clone());
        if let Some(q) = &quic_server {
            registry.register_all(q.metrics());
        }
        let server = MetricsServer::spawn(m.bind, Arc::new(registry))
            .await
            .with_context(|| format!("metrics on {}", m.bind))?;
        info!("metrics on {}", m.bind);
        Some(server)
    } else {
        None
    };

    if let Some(admin) = cfg.admin.clone() {
        let routes = admin_api::router(Arc::new(admin_api::Admin {
            gate: gate.clone(),
            enrol_dir: matches!(cfg.boxes, Boxes::Enrolled).then(|| cfg.state_dir.clone()),
            state_dir: cfg.state_dir.clone(),
            started,
            turn: turn.clone(),
            name: match &cfg.boxes {
                Boxes::ControlPlane { name, .. } => name.clone(),
                Boxes::Enrolled => String::new(),
            },
        }));
        let router = zoomriya_serve::admin::router(
            routes,
            zoomriya_serve::admin::Tokens::new(admin.tokens_file.clone()),
            &admin.web_dir,
        );
        tasks.spawn(async move {
            if let Err(err) = zoomriya_serve::admin::serve(&admin, router).await {
                error!("GUI: {err:#}");
            }
        });
    }

    info!(boxes = gate.boxes().len(), "relay running");
    tokio::select! {
        _ = zoomriya_serve::shutdown_signal() => info!("stopping"),
        Some(res) = tasks.join_next() => {
            if let Err(err) = res { error!("task ended: {err}") }
            anyhow::bail!("a part of the relay stopped");
        }
    }
    relay.shutdown().await;
    if let Some(q) = quic_server {
        let _ = q.shutdown().await;
    }
    Ok(())
}
