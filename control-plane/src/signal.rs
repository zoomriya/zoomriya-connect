//! Guests' setup messages: the control plane passes WebRTC setup messages
//! between a guest's browser and the box, never reading them and never
//! carrying the album (docs/sharing-and-names.md, "How a guest's browser
//! reaches the album").
//!
//! | Socket | Who | Proof |
//! |---|---|---|
//! | `GET /v1/signal/box` | The box, while it shares anything | Signed with the box key |
//! | `GET /v1/signal/guest/{name}` | The guest page | Comes from the guest page's own address (`Origin`) |
//!
//! The box gets `SignalEvent`s; guests send and get the message text as it is.
//! A guest's socket lasts two minutes at most: long enough to set up WebRTC.

#![allow(clippy::result_large_err)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    extract::{
        ConnectInfo, Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Method, Uri, header},
    response::{IntoResponse, Response},
};
use tokio::sync::mpsc;
use tracing::debug;
use zoomriya_proto::{SignalEvent, SignalMessage, names};

use crate::api::{Api, error, signed, slow_down};

/// Longest setup message.
pub const MAX_MESSAGE: usize = 16 * 1024;
/// Most messages from one guest.
const MAX_GUEST_MESSAGES: u32 = 100;
/// A guest's socket lasts this long at most.
const GUEST_LIFETIME: Duration = Duration::from_secs(120);
/// Most guests waiting on one box at once.
const MAX_GUESTS_PER_BOX: usize = 50;

struct BoxConn {
    id: u64,
    tx: mpsc::Sender<SignalEvent>,
}

struct GuestConn {
    name: String,
    tx: mpsc::Sender<String>,
}

/// Who is connected.
#[derive(Default)]
pub struct Hub {
    boxes: Mutex<HashMap<String, BoxConn>>,
    guests: Mutex<HashMap<u64, GuestConn>>,
    next: AtomicU64,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hub")
    }
}

impl Hub {
    fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn box_tx(&self, name: &str) -> Option<mpsc::Sender<SignalEvent>> {
        self.boxes
            .lock()
            .ok()
            .and_then(|b| b.get(name).map(|c| c.tx.clone()))
    }

    fn guests_of(&self, name: &str) -> usize {
        self.guests
            .lock()
            .map(|g| g.values().filter(|c| c.name == name).count())
            .unwrap_or(0)
    }

    /// Whether the box is connected now.
    pub fn box_online(&self, name: &str) -> bool {
        self.box_tx(name).is_some()
    }

    /// How many boxes and guests are connected.
    pub fn counts(&self) -> (usize, usize) {
        (
            self.boxes.lock().map(|b| b.len()).unwrap_or(0),
            self.guests.lock().map(|g| g.len()).unwrap_or(0),
        )
    }
}

/// `GET /v1/signal/box`
pub async fn box_socket(
    State(api): State<Arc<Api>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !api.per_addr.allow(&peer.ip()) {
        return slow_down();
    }
    let key = match signed(&api, &method, &uri, &headers, b"") {
        Ok(k) => k,
        Err(r) => return r,
    };
    let name = match api.store.find_by_key(&key) {
        Ok(Some(row)) if !row.stolen && !row.disabled => row.name,
        _ => return error(403, "unknown_box", "This box isn't registered here."),
    };
    ws.max_message_size(MAX_MESSAGE * 2)
        .on_upgrade(move |socket| run_box(api, name, socket))
}

async fn run_box(api: Arc<Api>, name: String, mut socket: WebSocket) {
    let hub = &api.hub;
    let (tx, mut rx) = mpsc::channel::<SignalEvent>(256);
    let id = hub.next_id();
    if let Ok(mut b) = hub.boxes.lock() {
        // A newer socket from the same box replaces the old one.
        b.insert(name.clone(), BoxConn { id, tx });
    }
    debug!(%name, "box connected for setup messages");
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else { break };
                let Ok(text) = serde_json::to_string(&event) else { continue };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => break,
                };
                let Ok(m) = serde_json::from_str::<SignalMessage>(&text) else { continue };
                if m.data.len() > MAX_MESSAGE {
                    continue;
                }
                // Only to this box's own guests.
                let guest = hub.guests.lock().ok().and_then(|g| {
                    g.get(&m.session)
                        .filter(|c| c.name == name)
                        .map(|c| c.tx.clone())
                });
                if let Some(g) = guest {
                    let _ = g.try_send(m.data);
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
        }
    }
    if let Ok(mut b) = hub.boxes.lock()
        && b.get(&name).is_some_and(|c| c.id == id)
    {
        b.remove(&name);
    }
    debug!(%name, "box left");
}

/// `GET /v1/signal/guest/{name}`
pub async fn guest_socket(
    State(api): State<Arc<Api>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if origin != api.share_origin {
        return error(
            403,
            "wrong_origin",
            "Open the share link from its own page.",
        );
    }
    if !api.per_guest.allow(&peer.ip()) {
        return slow_down();
    }
    let name = names::normalise(&name);
    if !api.hub.box_online(&name) {
        return error(
            404,
            "box_offline",
            "The box that shared this album can't be reached right now.",
        );
    }
    if api.hub.guests_of(&name) >= MAX_GUESTS_PER_BOX {
        return slow_down();
    }
    ws.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| run_guest(api, name, socket))
        .into_response()
}

async fn run_guest(api: Arc<Api>, name: String, mut socket: WebSocket) {
    let hub = &api.hub;
    let session = hub.next_id();
    let (tx, mut rx) = mpsc::channel::<String>(64);
    if let Ok(mut g) = hub.guests.lock() {
        g.insert(
            session,
            GuestConn {
                name: name.clone(),
                tx,
            },
        );
    }
    let deadline = tokio::time::sleep(GUEST_LIFETIME);
    tokio::pin!(deadline);
    let mut sent = 0u32;
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            data = rx.recv() => {
                let Some(data) = data else { break };
                if socket.send(Message::Text(data.into())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => break,
                };
                sent += 1;
                if sent > MAX_GUEST_MESSAGES || text.len() > MAX_MESSAGE {
                    break;
                }
                let Some(b) = hub.box_tx(&name) else { break };
                let event = SignalEvent::Message(SignalMessage { session, data: text.to_string() });
                if b.try_send(event).is_err() {
                    break;
                }
            }
        }
    }
    if let Ok(mut g) = hub.guests.lock() {
        g.remove(&session);
    }
    if let Some(b) = hub.box_tx(&name) {
        let _ = b.try_send(SignalEvent::Gone { session });
    }
}
