//! `zoomriya-connectd`: the box's side of ZOOMRIYA Connect. The only part
//! that faces outside, and where every "who gets in" decision is made.
//!
//! ```text
//! zoomriya-connectd run    [--config /etc/zoomriya-connect/connectd.toml]
//! zoomriya-connectd status [--config ...]
//! ```

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use anyhow::Result;
use tracing::info;

mod acme;
mod api;
mod clock;
mod config;
mod daemon;
mod devices;
mod files;
mod laptop;
mod link;
mod net;
mod relays;
mod server;
mod state;

#[cfg(test)]
mod e2e;

/// Seconds since 1970.
pub fn now() -> u64 {
    zoomriya_serve::now()
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
    let st = ctx.state.get();
    println!("Box key:  {}", ctx.box_key.public().to_z32());
    println!("Server:   {}", ctx.server_url());
    println!(
        "Name:     {}",
        st.fqdn.as_deref().unwrap_or("not chosen yet")
    );
    println!("On:       {}", if st.off { "no" } else { "yes" });
    let devices = ctx.devices.all();
    let paired = devices.iter().filter(|d| d.removed_at.is_none()).count();
    println!("Devices:  {paired} paired");
    Ok(())
}

fn run(path: PathBuf) -> Result<()> {
    zoomriya_serve::init_logging();
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
                _ = daemon::run(ctx) => {}
                r = api => { r??; }
                _ = zoomriya_serve::shutdown_signal() => info!("stopping"),
            }
            Ok(())
        })
}
