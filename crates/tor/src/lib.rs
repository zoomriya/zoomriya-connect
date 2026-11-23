//! Tor for Direct+ (docs/direct-plus.md §3), through Arti. No ZOOMRIYA
//! server: the box's own iroh relay waits behind an onion service, and the
//! apps reach it through Tor when no direct path works.
//!
//! | Side | What |
//! |---|---|
//! | The box | [`Tor::serve`]: an onion service with a stable address, forwarding port 80 to the box's iroh relay |
//! | The apps | [`proxy`]: a CONNECT proxy on 127.0.0.1 for iroh, reaching only the box's onion address |

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result};
use arti_client::{BootstrapBehavior, TorClient, TorClientConfig, config::TorClientConfigBuilder};
use futures::StreamExt;
use safelog::DisplayRedacted;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tor_cell::relaycell::msg::{Connected, End};
use tor_hsservice::{
    RunningOnionService, StreamRequest, config::OnionServiceConfigBuilder, handle_rend_requests,
};
use tor_rtcompat::PreferredRuntime;
use tracing::{debug, info};

/// The port the onion service answers on.
pub const ONION_PORT: u16 = 80;
/// Longest CONNECT request the proxy reads.
const MAX_REQUEST: usize = 4096;

/// A Tor client. Cheap to clone.
#[derive(Clone)]
pub struct Tor {
    client: Arc<TorClient<PreferredRuntime>>,
}

impl std::fmt::Debug for Tor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tor")
    }
}

impl Tor {
    /// A Tor client keeping its keys and state in `state` and what it
    /// downloads in `cache`. It joins the Tor network in the background, and
    /// anything asked of it waits until it has.
    pub fn start(state: &Path, cache: &Path) -> Result<Self> {
        let config: TorClientConfig = TorClientConfigBuilder::from_directories(state, cache)
            .build()
            .context("Tor settings")?;
        let client = TorClient::builder()
            .config(config)
            .bootstrap_behavior(BootstrapBehavior::OnDemand)
            .create_unbootstrapped()
            .context("starting Tor")?;
        let c = client.clone();
        tokio::spawn(async move {
            match c.bootstrap().await {
                Ok(()) => info!("joined the Tor network"),
                Err(e) => info!("Tor: {e}"),
            }
        });
        Ok(Self { client })
    }

    /// An onion service named `nickname`, forwarding [`ONION_PORT`] to `to`.
    /// Its key is kept in the state folder, so the address never changes.
    pub fn serve(&self, nickname: &str, to: SocketAddr) -> Result<Onion> {
        let config = OnionServiceConfigBuilder::default()
            .nickname(nickname.parse().context("onion service name")?)
            .build()
            .context("onion service settings")?;
        let (service, requests) = self
            .client
            .launch_onion_service(config)
            .context("starting the onion service")?
            .context("onion service turned off")?;
        let address = service
            .onion_address()
            .context("onion service has no address")?
            .display_unredacted()
            .to_string();
        let task = tokio::spawn(async move {
            let mut streams = std::pin::pin!(handle_rend_requests(requests));
            while let Some(req) = streams.next().await {
                tokio::spawn(forward(req, to));
            }
        });
        info!("onion service started");
        Ok(Onion {
            address,
            _service: service,
            task,
        })
    }
}

/// A running onion service. Dropping it stops the service.
pub struct Onion {
    /// Its address, `….onion`.
    pub address: String,
    _service: Arc<RunningOnionService>,
    task: JoinHandle<()>,
}

impl Drop for Onion {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl std::fmt::Debug for Onion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Onion")
    }
}

async fn forward(req: StreamRequest, to: SocketAddr) {
    use tor_proto::stream::IncomingStreamRequest;
    let port = match req.request() {
        IncomingStreamRequest::Begin(b) => b.port(),
        _ => 0,
    };
    if port != ONION_PORT {
        let _ = req.reject(End::new_misc()).await;
        return;
    }
    let local = match TcpStream::connect(to).await {
        Ok(s) => s,
        Err(e) => {
            debug!("onion service: {e}");
            let _ = req.reject(End::new_misc()).await;
            return;
        }
    };
    let Ok(mut tor) = req.accept(Connected::new_empty()).await else {
        return;
    };
    let mut local = local;
    let _ = tokio::io::copy_bidirectional(&mut tor, &mut local).await;
}

/// A CONNECT proxy on 127.0.0.1 for iroh's relay client, reaching only
/// `onion` on [`ONION_PORT`] through Tor. Returns its address, to give iroh
/// as `http://<address>`.
pub async fn proxy(tor: Tor, onion: String) -> Result<(SocketAddr, JoinHandle<()>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let onion = Arc::new(onion);
    let task = tokio::spawn(async move {
        while let Ok((conn, _)) = listener.accept().await {
            let tor = tor.clone();
            let onion = onion.clone();
            tokio::spawn(async move {
                if let Err(e) = tunnel(conn, &tor, &onion).await {
                    debug!("Tor proxy: {e:#}");
                }
            });
        }
    });
    Ok((addr, task))
}

/// The target of a CONNECT request, if it is `onion` on [`ONION_PORT`].
fn connect_target(head: &str, onion: &str) -> bool {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some("CONNECT"), Some(target)) = (parts.next(), parts.next()) else {
        return false;
    };
    target.eq_ignore_ascii_case(&format!("{onion}:{ONION_PORT}"))
}

async fn tunnel(mut conn: TcpStream, tor: &Tor, onion: &str) -> Result<()> {
    let mut buf = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        anyhow::ensure!(buf.len() < MAX_REQUEST, "request too long");
        anyhow::ensure!(conn.read(&mut byte).await? == 1, "closed");
        buf.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&buf);
    if !connect_target(&head, onion) {
        conn.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        anyhow::bail!("refused a CONNECT elsewhere");
    }
    let mut stream = match tor.client.connect((onion, ONION_PORT)).await {
        Ok(s) => s,
        Err(e) => {
            conn.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await?;
            return Err(e.into());
        }
    };
    conn.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
    tokio::io::copy_bidirectional(&mut conn, &mut stream).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_box() {
        let o = "abc.onion";
        assert!(connect_target(
            "CONNECT abc.onion:80 HTTP/1.1\r\nHost: x\r\n\r\n",
            o
        ));
        assert!(!connect_target("CONNECT abc.onion:443 HTTP/1.1\r\n\r\n", o));
        assert!(!connect_target(
            "CONNECT example.com:80 HTTP/1.1\r\n\r\n",
            o
        ));
        assert!(!connect_target("GET http://abc.onion/ HTTP/1.1\r\n\r\n", o));
    }
}
