//! The ZOOMRIYA laptop app's core. The app only connects the laptop to the
//! box; the laptop's own browser is where everything happens
//! (docs/direct-plus.md, §10). The tray shell (Tauri) and the command line
//! both use this.
//!
//! | Module | What |
//! |---|---|
//! | [`store`] | Keys and the paired box, in the user's app data folder |
//! | [`serve`] | The box at `zoomriya.localhost:7480` in the browser |

pub mod serve;
pub mod store;

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use tracing::info;
pub use zoomriya_link::{client::Client, pair::PairError};
use zoomriya_link::{find, pair};

use crate::{
    serve::{Io, Opening, Server, Upstream},
    store::{PairedBox, Store},
};

/// How long to listen for boxes on the home network when pairing.
const LOOK: Duration = Duration::from_secs(4);

/// Requests to the box over iroh.
#[derive(Debug, Clone)]
pub struct ToTheBox(pub Arc<Client>);

impl Upstream for ToTheBox {
    fn open(&self, target: &'static str) -> Opening {
        let c = self.0.clone();
        Box::pin(async move {
            let (send, recv) = c.web(target).await?;
            Ok(Box::new(tokio::io::join(recv, send)) as Box<dyn Io>)
        })
    }
}

/// What the laptop calls itself when pairing, unless Box Desktop named it.
pub fn laptop_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "Laptop".into())
}

/// Pairs with the box showing `code` on the home network, and remembers it.
pub async fn pair_at_home(store: &Store, code: &str) -> Result<PairedBox, PairError> {
    let found = find::boxes(LOOK)
        .await
        .map_err(|_| PairError::Unreachable)?;
    if found.is_empty() {
        return Err(PairError::Unreachable);
    }
    let link = store.link_key().map_err(|_| PairError::Broken)?;
    let device = store.device_key().map_err(|_| PairError::Broken)?;
    let ep = zoomriya_link::endpoint(link)
        .await
        .map_err(|_| PairError::Broken)?;
    let name = laptop_name();
    // Usually one box. With several, only the one showing a code answers.
    let mut last = PairError::Unreachable;
    for b in found {
        match pair::laptop(&ep, b.key, &b.addrs, code, &device, &name).await {
            Ok(p) => {
                let paired = PairedBox {
                    key: p.box_key.into(),
                    addrs: p.addrs,
                };
                store.save_paired(&paired).map_err(|_| PairError::Broken)?;
                info!(box_key = %p.box_key.fmt_short(), "paired");
                ep.close().await;
                return Ok(paired);
            }
            Err(e) => last = e,
        }
    }
    ep.close().await;
    Err(last)
}

/// The paired box's client, saving its addresses as they change.
pub async fn client(store: &Store) -> Result<Arc<Client>> {
    let paired = store
        .paired()?
        .context("this laptop is not paired with a box yet")?;
    let ep = zoomriya_link::endpoint(store.link_key()?).await?;
    let s = store.clone();
    let key = paired.key;
    Ok(Arc::new(Client::new(
        ep,
        paired.key.0,
        paired.addrs,
        Arc::new(move |addrs| {
            let _ = s.save_paired(&PairedBox {
                key,
                addrs: addrs.to_vec(),
            });
        }),
    )))
}

/// The browser server for the paired box.
pub async fn server(store: &Store, port: u16) -> Result<(Arc<Client>, Arc<Server<ToTheBox>>)> {
    let client = client(store).await?;
    let server = Server::new(ToTheBox(client.clone()), store.browser_secret()?, port);
    Ok((client, server))
}

/// Opens `url` in the laptop's default browser.
pub fn open_in_browser(url: &str) -> Result<()> {
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    cmd.arg(url).spawn().context("opening the browser")?;
    Ok(())
}
