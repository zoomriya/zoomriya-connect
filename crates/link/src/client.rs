//! The connection to a paired box: iroh, pinned to the box key, over the
//! home network, the box's direct addresses, or the box's own relay through
//! Tor. One connection, reopened when it drops; one stream per web request
//! ([`Client::web`]).
//!
//! Two tries race; the first to connect wins:
//!
//! | Try | Addresses |
//! |---|---|
//! | Remembered | From pairing, the box's welcome, earlier lookups; and the box's relay through Tor, if it has one |
//! | Looked up | The home network (mDNS) and the box's record in the DHT |
//!
//! Through the relay, iroh punches a direct path and moves to it without
//! dropping the connection.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointAddr, RelayUrl, TransportAddr,
    endpoint::{Connection, RecvStream, SendStream},
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, info};

use crate::{dht::Dht, find};
use zoomriya_keys::{PublicKey, record::RecordSecret};
use zoomriya_proto::link::{FromBox, LINK_ALPN, MAX_LINE, STREAM_CONTROL, STREAM_WEB, ToBox};

/// The link's protocol version.
const VERSION: u32 = 1;
/// Most addresses kept for the box.
const MAX_ADDRS: usize = 16;
/// How long one try to reach the box may take.
const CONNECT: Duration = Duration::from_secs(10);
/// How long the remembered addresses get before looking for new ones.
const KNOWN: Duration = Duration::from_secs(5);
/// How long to listen on the home network.
const LOOK: Duration = Duration::from_secs(3);
/// How long a try through Tor may take: joining the Tor network, then
/// reaching the onion service.
const THROUGH_TOR: Duration = Duration::from_secs(90);

/// Called with the box's addresses whenever they change, so the app can
/// save them.
pub type OnAddrs = Arc<dyn Fn(&[SocketAddr]) + Send + Sync>;

/// How the connection to the box runs now, for the apps to adapt
/// (docs/direct-plus.md §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// Not connected.
    Down,
    /// Straight to the box: full speed.
    Direct,
    /// Through the box's relay over Tor: send less.
    Relay,
}

impl Path {
    /// Its name, as the browser sees it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Direct => "direct",
            Self::Relay => "relay",
        }
    }
}

/// After a failed try, the wait before the next, doubling up to the last.
const RETRY: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

type Dialling<'a> = std::pin::Pin<Box<dyn Future<Output = Result<Connection>> + Send + 'a>>;

/// A paired box, and the connection to it.
pub struct Client {
    ep: Endpoint,
    box_key: PublicKey,
    addrs: Arc<Mutex<Vec<SocketAddr>>>,
    conn: tokio::sync::Mutex<Option<Connection>>,
    on_addrs: OnAddrs,
    record: Option<RecordSecret>,
    relay: Option<RelayUrl>,
    dht: tokio::sync::OnceCell<Dht>,
    path: tokio::sync::watch::Sender<Path>,
    keeper: Mutex<Option<tokio::task::JoinHandle<()>>>,
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
    /// `record`: the secret behind the box's DHT record, from pairing.
    /// `relay`: the box's relay through Tor, if `ep` reaches it
    /// ([`crate::endpoint_via`]).
    pub fn new(
        ep: Endpoint,
        box_key: PublicKey,
        addrs: Vec<SocketAddr>,
        record: Option<RecordSecret>,
        relay: Option<RelayUrl>,
        on_addrs: OnAddrs,
    ) -> Self {
        Self {
            ep,
            box_key,
            addrs: Arc::new(Mutex::new(addrs)),
            conn: tokio::sync::Mutex::new(None),
            on_addrs,
            record,
            relay,
            dht: tokio::sync::OnceCell::new(),
            path: tokio::sync::watch::Sender::new(Path::Down),
            keeper: Mutex::new(None),
        }
    }

    /// How the connection runs now.
    pub fn path(&self) -> Path {
        *self.path.borrow()
    }

    /// Changes of [`Self::path`].
    pub fn path_changes(&self) -> tokio::sync::watch::Receiver<Path> {
        self.path.subscribe()
    }

    /// Keeps a connection up from now on, reconnecting when it drops: the
    /// way through Tor is ready before the first request, not built on it.
    /// [`Self::close`] stops it.
    pub fn keep_connected(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            let mut failed = 0;
            loop {
                let Some(c) = me.upgrade() else { return };
                match c.connection().await {
                    Ok(conn) => {
                        failed = 0;
                        drop(c);
                        conn.closed().await;
                        debug!("connection to the box dropped");
                    }
                    Err(e) => {
                        drop(c);
                        debug!("box not reached: {e:#}");
                        tokio::time::sleep(RETRY[failed]).await;
                        failed = (failed + 1).min(RETRY.len() - 1);
                    }
                }
            }
        });
        if let Ok(mut k) = self.keeper.lock()
            && let Some(old) = k.replace(task)
        {
            old.abort();
        }
    }

    /// Follows `conn`'s paths into [`Self::path`] until it closes.
    fn watch_path(&self, conn: Connection) {
        let path = self.path.clone();
        tokio::spawn(async move {
            use futures::StreamExt;
            let mut paths = conn.paths_stream();
            while let Some(list) = paths.next().await {
                let direct = list.iter().any(|p| p.is_selected() && p.is_ip());
                path.send_replace(if direct { Path::Direct } else { Path::Relay });
            }
            path.send_replace(Path::Down);
        });
    }

    /// The box's addresses from its record in the DHT, if it has one.
    async fn look_in_dht(&self) -> Vec<SocketAddr> {
        let Some(secret) = &self.record else {
            return Vec::new();
        };
        let dht = match self.dht.get_or_try_init(|| async { Dht::start() }).await {
            Ok(d) => d,
            Err(e) => {
                debug!("DHT: {e:#}");
                return Vec::new();
            }
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match dht.lookup(secret, now).await {
            Some((addrs, at)) => {
                debug!(?addrs, at, "the box's record");
                addrs
            }
            None => Vec::new(),
        }
    }

    /// Closes the connection and the endpoint, telling the box.
    pub async fn close(&self) {
        if let Some(k) = self.keeper.lock().ok().and_then(|mut k| k.take()) {
            k.abort();
        }
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
        let to = |addrs: Vec<SocketAddr>, relay: bool| {
            let relay = self
                .relay
                .clone()
                .filter(|_| relay)
                .map(TransportAddr::Relay);
            EndpointAddr::from_parts(
                self.box_key,
                addrs.into_iter().map(TransportAddr::Ip).chain(relay),
            )
        };
        let known = self.addrs();
        let remembered = async {
            anyhow::ensure!(
                !known.is_empty() || self.relay.is_some(),
                "no address remembered"
            );
            let wait = if self.relay.is_some() {
                THROUGH_TOR
            } else {
                KNOWN
            };
            Ok(
                tokio::time::timeout(wait, self.ep.connect(to(known.clone(), true), LINK_ALPN))
                    .await
                    .context("the box could not be reached")??,
            )
        };
        let looked_up = async {
            let (home, away) = tokio::join!(find::the_box(&self.box_key, LOOK), self.look_in_dht());
            let mut found = home.unwrap_or_default();
            found.extend(away);
            found.retain(|a| !known.contains(a));
            anyhow::ensure!(!found.is_empty(), "the box could not be found");
            debug!(?found, "the box is somewhere new");
            self.note_addrs(&found);
            Ok(
                tokio::time::timeout(CONNECT, self.ep.connect(to(found, false), LINK_ALPN))
                    .await
                    .context("the box could not be reached")??,
            )
        };
        let (conn, _) =
            futures::future::select_ok([Box::pin(remembered) as Dialling<'_>, Box::pin(looked_up)])
                .await?;
        Ok(conn)
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
        self.watch_path(conn.clone());
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
