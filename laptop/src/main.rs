//! `zoomriya-laptop`: the laptop app from the command line, until the tray
//! shell is built.
//!
//! ```text
//! zoomriya-laptop pair CODE      pair at home with the code Box Desktop shows
//! zoomriya-laptop run [--no-open] connect, and serve the box to the browser
//! zoomriya-laptop status         paired or not
//! zoomriya-laptop forget         forget the box
//! ```

use std::process::ExitCode;

use anyhow::Result;
use zoomriya_laptop::{
    serve::{PORT, Site},
    store::{Store, default_dir},
};

const USAGE: &str = "usage: zoomriya-laptop (pair CODE | run [--no-open] | status | forget)";

#[tokio::main]
async fn main() -> ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["pair", code] => pair(code).await,
        ["run"] => run(true).await,
        ["run", "--no-open"] => run(false).await,
        ["status"] => status(),
        ["forget"] => forget(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn store() -> Result<Store> {
    Ok(Store::new(default_dir()?))
}

async fn pair(code: &str) -> Result<()> {
    let s = store()?;
    if s.paired()?.is_some() {
        anyhow::bail!("This laptop is already paired. Run `zoomriya-laptop forget` first.");
    }
    zoomriya_laptop::pair_at_home(&s, code).await?;
    println!("Paired. Run `zoomriya-laptop run` to open ZOOMRIYA.");
    Ok(())
}

async fn run(open: bool) -> Result<()> {
    let s = store()?;
    let (client, server) = zoomriya_laptop::server(&s, PORT).await?;
    // Connect now, so the first page opens at once.
    if let Err(e) = client.connection().await {
        eprintln!("The box cannot be reached yet ({e:#}). Still trying.");
    }
    let url = server.launch_url(Site::Apps, "/")?;
    println!("Open ZOOMRIYA: {url}");
    println!(
        "Box Desktop:   http://{}:{PORT}/",
        zoomriya_laptop::serve::DESKTOP_HOST
    );
    if open {
        zoomriya_laptop::open_in_browser(&url)?;
    }
    tokio::select! {
        r = server.run() => r,
        _ = tokio::signal::ctrl_c() => Ok(()),
    }
}

fn status() -> Result<()> {
    match store()?.paired()? {
        Some(b) => println!("Paired with the box {}", b.key),
        None => println!("Not paired"),
    }
    Ok(())
}

fn forget() -> Result<()> {
    store()?.forget()?;
    println!("Forgotten. Remove this laptop in Box Desktop too: Control Panel › Devices.");
    Ok(())
}
