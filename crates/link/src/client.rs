//! The connection to a paired box: iroh, pinned to the box key, over the
//! home network or the box's direct addresses. One connection, reopened
//! when it drops; one stream per web request ([`Client::web`]).

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointAddr, TransportAddr,
    endpoint::{Connection, RecvStream, SendStream},
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, info};
use zoomriya_keys::PublicKey;
use zoomriya_proto::link::{FromBox, LINK_ALPN, MAX_LINE, STREAM_CONTROL, STREAM_WEB, ToBox};

/// The link's protocol version.
const VERSION: u32 = 1;
/// Most addresses kept for the box.
const MAX_ADDRS: usize = 16;
/// How long one try to reach the box may take.
const CONNECT: Duration = Duration::from_secs(10);
/// How long to listen on the home network when the saved addresses fail.
const LOOK: Duration = Duration::from_secs(3);

/// Called with the box's addresses whenever they change, so the app can
/// save them.
pub type OnAddrs = Arc<dyn Fn(&[SocketAddr]) + Send + Sync>;

/// A paired box, and the connection to it.
pub struct Client {
    ep: Endpoint,
    box_key: PublicKey,
    addrs: Arc<Mutex<Vec<SocketAddr>>>,
    conn: tokio::sync::Mutex<Option<Connection>>,
    on_addrs: OnAddrs,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("box_key", &self.box_key.fmt_short().to_string())
            .finish()
    }
}

fn merge(into: &mut Vec<SocketAddr>, new: &[SocketAddr]) -> bool {
    let before = into.clone();
    for a in new {
        if !into.contains(a) {
            into.insert(0, *a);
        }
    }
    into.truncate(MAX_ADDRS);
    *into != before
}

impl Client {
    /// A client for the box at `box_key`, known at `addrs`. `ep` uses the
    /// device's link key.
    pub fn new(
        ep: Endpoint,
        box_key: PublicKey,
        addrs: Vec<SocketAddr>,
        on_addrs: OnAddrs,
    ) -> Self {
        Self {
            ep,
            box_key,
            addrs: Arc::new(Mutex::new(addrs)),
            conn: tokio::sync::Mutex::new(None),
            on_addrs,
        }
    }

    /// Closes the connection and the endpoint, telling the box.
    pub async fn close(&self) {
        if let Some(c) = self.conn.lock().await.take() {
            c.close(0u32.into(), b"bye");
        }
        self.ep.close().await;
    }

    /// The box's addresses as known now.
    pub fn addrs(&self) -> Vec<SocketAddr> {
        self.addrs.lock().map(|a| a.clone()).unwrap_or_default()
    }

    /// Whether a connection is up now.
    pub async fn connected(&self) -> bool {
        self.conn
            .lock()
            .await
            .as_ref()
            .is_some_and(|c| c.close_reason().is_none())
    }

    fn note_addrs(&self, new: &[SocketAddr]) {
        let changed = self
            .addrs
            .lock()
            .map(|mut a| merge(&mut a, new))
            .unwrap_or(false);
        if changed {
            (self.on_addrs)(&self.addrs());
        }
    }

    async fn dial(&self) -> Result<Connection> {
        let to = |addrs: Vec<SocketAddr>| {
            EndpointAddr::from_parts(self.box_key, addrs.into_iter().map(TransportAddr::Ip))
        };
        let first = tokio::time::timeout(CONNECT, self.ep.connect(to(self.addrs()), LINK_ALPN))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|r| r.map_err(anyhow::Error::from));
        match first {
            Ok(c) => Ok(c),
            Err(e) => {
                // Moved on the home network? Listen for it, then try again.
                debug!("box not reached at known addresses: {e:#}");
                let found = crate::find::the_box(&self.box_key, LOOK).await?;
                anyhow::ensure!(!found.is_empty(), "the box could not be reached");
                self.note_addrs(&found);
                Ok(
                    tokio::time::timeout(CONNECT, self.ep.connect(to(found), LINK_ALPN))
                        .await
                        .context("the box could not be reached")??,
                )
            }
        }
    }

    /// The connection, reopened if it dropped.
    pub async fn connection(&self) -> Result<Connection> {
        let mut slot = self.conn.lock().await;
        if let Some(c) = slot.as_ref()
            && c.close_reason().is_none()
        {
            return Ok(c.clone());
        }
        let conn = self.dial().await?;
        info!(box_key = %self.box_key.fmt_short(), "connected to the box");
        self.control(&conn).await?;
        *slot = Some(conn.clone());
        Ok(conn)
    }

    /// Says hello, and keeps listening for the box's addresses.
    async fn control(&self, conn: &Connection) -> Result<()> {
        let (mut send, recv) = conn.open_bi().await?;
        let mut hello = vec![STREAM_CONTROL];
        hello.extend(serde_json::to_vec(&ToBox::Hello { version: VERSION })?);
        hello.push(b'\n');
        send.write_all(&hello).await?;
        let addrs = self.addrs.clone();
        let on_addrs = self.on_addrs.clone();
        tokio::spawn(async move {
            let _send = send; // closing it would end the control stream
            let mut recv = BufReader::new(recv);
            let mut line = String::new();
            loop {
                line.clear();
                match recv.read_line(&mut line).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) if line.len() > MAX_LINE => return,
                    Ok(_) => {}
                }
                let new = match serde_json::from_str::<FromBox>(&line) {
                    Ok(FromBox::Welcome { addrs } | FromBox::Addrs { addrs }) => addrs,
                    Ok(FromBox::Refused { message, .. }) => {
                        info!("the box says: {message}");
                        continue;
                    }
                    _ => continue,
                };
                let changed = addrs
                    .lock()
                    .map(|mut a| merge(&mut a, &new))
                    .unwrap_or(false);
                if changed && let Ok(a) = addrs.lock().map(|a| a.clone()) {
                    on_addrs(&a);
                }
            }
        });
        Ok(())
    }

    /// A stream to one of the box's web servers by its name, such as `apps`
    /// or `desktop`.
    pub async fn web(&self, target: &str) -> Result<(SendStream, RecvStream)> {
        anyhow::ensure!(target.len() < 256, "target name too long");
        let conn = self.connection().await?;
        let (mut send, recv) = match conn.open_bi().await {
            Ok(s) => s,
            Err(_) => {
                // Dropped just now: once more, on a fresh connection.
                if let Ok(mut slot) = self.conn.try_lock() {
                    *slot = None;
                }
                self.connection().await?.open_bi().await?
            }
        };
        let mut head = vec![STREAM_WEB, target.len() as u8];
        head.extend_from_slice(target.as_bytes());
        send.write_all(&head).await?;
        Ok((send, recv))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_addresses_first_and_capped() {
        let a = |p: u16| SocketAddr::from(([192, 168, 1, 20], p));
        let mut known = vec![a(1)];
        assert!(merge(&mut known, &[a(2), a(1)]));
        assert_eq!(known, vec![a(2), a(1)]);
        assert!(!merge(&mut known, &[a(1)]));
        let many: Vec<_> = (10..40).map(a).collect();
        merge(&mut known, &many);
        assert_eq!(known.len(), MAX_ADDRS);
    }
}
