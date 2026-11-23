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

    fn path(&self) -> &'static str {
        self.0.path().as_str()
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
                    record: p.record.as_ref().map(|r| r.to_b64()),
                    onion: p.onion.clone(),
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
    let (ep, relay) = endpoint(store, &paired).await?;
    let s = store.clone();
    let key = paired.key;
    let record_text = paired.record.clone();
    let onion = paired.onion.clone();
    let record = paired
        .record
        .as_deref()
        .and_then(|r| zoomriya_keys::record::RecordSecret::from_b64(r).ok());
    Ok(Arc::new(Client::new(
        ep,
        paired.key.0,
        paired.addrs,
        record,
        relay,
        Arc::new(move |addrs| {
            let _ = s.save_paired(&PairedBox {
                key,
                addrs: addrs.to_vec(),
                record: record_text.clone(),
                onion: onion.clone(),
            });
        }),
    )))
}

/// The laptop's endpoint: through Tor to the box's relay if the box has an
/// onion address, otherwise direct only.
async fn endpoint(
    store: &Store,
    paired: &PairedBox,
) -> Result<(iroh::Endpoint, Option<iroh::RelayUrl>)> {
    let link = store.link_key()?;
    let Some(onion) = paired.onion.clone() else {
        return Ok((zoomriya_link::endpoint(link).await?, None));
    };
    let (state, cache) = store.tor_dirs()?;
    let bridges = store.bridges()?;
    let tor = zoomriya_tor::Tor::start(&state, &cache, bridges.as_ref())?;
    let (proxy, _) = zoomriya_tor::proxy(tor, onion.clone()).await?;
    let (ep, relay) = zoomriya_link::endpoint_via(link, &onion, proxy).await?;
    Ok((ep, Some(relay)))
}

/// The browser server for the paired box.
pub async fn server(store: &Store, port: u16) -> Result<(Arc<Client>, Arc<Server<ToTheBox>>)> {
    let client = client(store).await?;
    client.keep_connected();
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

/// What [`check_away`] found.
#[derive(Debug)]
pub struct AwayCheck {
    /// The addresses in the box's DHT record, and when it wrote them.
    pub record: Option<(Vec<std::net::SocketAddr>, u64)>,
    /// Whether the box answered at one of them.
    pub reached: bool,
}

/// What [`check_tor`] found.
#[derive(Debug)]
pub struct TorCheck {
    /// How long reaching the box through its relay took.
    pub connected_in: Duration,
    /// How long until iroh moved to a direct path, if it did.
    pub direct_after: Option<Duration>,
}

/// Reaches the box only through its relay over Tor, as from a home with no
/// direct path, then watches for iroh to punch through and move to a direct
/// path.
pub async fn check_tor(store: &Store, watch: Duration) -> Result<TorCheck> {
    let paired = store.paired()?.context("this laptop is not paired")?;
    anyhow::ensure!(
        paired.onion.is_some(),
        "the box gave no onion address at pairing: pair again"
    );
    let (ep, relay) = endpoint(store, &paired).await?;
    let relay = relay.context("no relay")?;
    let start = std::time::Instant::now();
    let to = iroh::EndpointAddr::from_parts(paired.key.0, [iroh::TransportAddr::Relay(relay)]);
    let conn = match tokio::time::timeout(
        Duration::from_secs(120),
        ep.connect(to, zoomriya_proto::link::LINK_ALPN),
    )
    .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            ep.close().await;
            return Err(e).context("the box refused this laptop or could not be reached");
        }
        Err(_) => {
            ep.close().await;
            anyhow::bail!("the box did not answer through Tor");
        }
    };
    let connected_in = start.elapsed();
    let mut direct_after = None;
    let until = std::time::Instant::now() + watch;
    while std::time::Instant::now() < until {
        if conn.paths().iter().any(|p| p.is_selected() && p.is_ip()) {
            direct_after = Some(start.elapsed() - connected_in);
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    conn.close(0u32.into(), b"bye");
    ep.close().await;
    Ok(TorCheck {
        connected_in,
        direct_after,
    })
}

/// Reaches the box the way a laptop away from home would: only through
/// its record in the DHT, never the home network or remembered addresses.
pub async fn check_away(store: &Store) -> Result<AwayCheck> {
    use zoomriya_keys::record::RecordSecret;
    let paired = store.paired()?.context("this laptop is not paired")?;
    let secret = paired
        .record
        .as_deref()
        .and_then(|r| RecordSecret::from_b64(r).ok())
        .context("the box gave no record secret at pairing: pair again")?;
    let dht = zoomriya_link::dht::Dht::start()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let record = dht.lookup(&secret, now).await;
    let mut reached = false;
    if let Some((addrs, _)) = &record
        && !addrs.is_empty()
    {
        let ep = zoomriya_link::endpoint(store.link_key()?).await?;
        let to = iroh::EndpointAddr::from_parts(
            paired.key.0,
            addrs.iter().map(|a| iroh::TransportAddr::Ip(*a)),
        );
        reached = matches!(
            tokio::time::timeout(
                Duration::from_secs(10),
                ep.connect(to, zoomriya_proto::link::LINK_ALPN)
            )
            .await,
            Ok(Ok(_))
        );
        ep.close().await;
    }
    Ok(AwayCheck { record, reached })
}
