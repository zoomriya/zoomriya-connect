//! `zoomriya-connectd`: the box's side of Direct+. The only part that faces
//! outside, and where every "who gets in" decision is made, from the box's
//! own list of paired devices. No ZOOMRIYA server is involved
//! (docs/direct-plus.md).
//!
//! ```text
//! zoomriya-connectd run    [--config /etc/zoomriya-connect/connectd.toml]
//! zoomriya-connectd status [--config ...]
//! ```

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use tracing::info;

mod api;
mod config;
mod daemon;
mod devices;
mod files;
mod link;
mod net;

#[cfg(test)]
mod e2e;

/// Seconds since 1970.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const USAGE: &str = "usage: zoomriya-connectd (run | status) [--config FILE]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, config) = match args.as_slice() {
        [c] => (
            c.as_str(),
            PathBuf::from("/etc/zoomriya-connect/connectd.toml"),
        ),
        [c, flag, path] if flag == "--config" => (c.as_str(), PathBuf::from(path)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match command {
        "run" => run(config),
        "status" => status(config),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zoomriya-connectd: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn status(path: PathBuf) -> Result<()> {
    let cfg = config::Config::load(&path)?;
    let ctx = daemon::Ctx::open(cfg)?;
    println!("Box key:  {}", ctx.box_key.public().to_z32());
    println!("Devices:  {} paired", ctx.devices.all().len());
    Ok(())
}

/// Logs to standard error, filtered by `RUST_LOG` (default `info`). systemd's
/// journal picks it up.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

/// Waits for Ctrl-C or SIGTERM.
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

fn run(path: PathBuf) -> Result<()> {
    init_logging();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cfg = config::Config::load(&path)?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let socket = cfg.api_socket.clone();
            let ctx = Arc::new(daemon::Ctx::open(cfg)?);
            info!(box_key = %ctx.box_key.public().fmt_short(), "connectd starting");
            let api = tokio::spawn(api::serve(socket, ctx.clone()));
            tokio::select! {
                r = daemon::run(ctx, shutdown_signal()) => { r?; }
                r = api => { r??; }
            }
            Ok(())
        })
}
