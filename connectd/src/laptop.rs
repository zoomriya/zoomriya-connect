//! Laptops: the browser extension reaches the box over WebRTC
//! (docs/laptops.md, `zoomriya_proto::laptop`).
//!
//! | Part | What |
//! |---|---|
//! | Home endpoint (TCP 7445, home network only) | `POST /laptop/pair`: pairing with Box Desktop's short code. `POST /laptop/connect`: a signed WebRTC offer, answered with the box's sealed answer |
//! | WebRTC (UDP 7446, one socket for every laptop) | str0m, with the data channel `zoomriya` |
//! | From away (the relays) | connectd waits on each relay for offers (`POST /zoomriya/v1/laptop/wait`), answers them as at home, and sends the answer back. STUN on the WebRTC port tells it the box's public address to offer |
//! | Forwarding | Each request on the data channel goes to the box's web server (Box Desktop), labelled with the laptop like a phone's, and its answer streams back, live event streams included |
//!
//! The laptop proves who it is by signing its offer with its device key,
//! pinned at pairing. The box seals its answer with the box key, which the
//! extension pinned: the answer carries the DTLS fingerprint, so WebRTC's
//! encryption ends at the real box and nowhere else.

use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use bytes::Bytes;
use data_encoding::BASE64URL_NOPAD;
use http_body_util::{BodyExt, StreamBody, combinators::BoxBody};
use hyper::body::Frame as BodyFrame;
use sha2::{Digest, Sha256};
use str0m::{
    Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig,
    change::SdpOffer,
    channel::ChannelId,
    net::{Protocol, Receive},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::UdpSocket,
    sync::{mpsc, oneshot},
};
use tracing::{debug, info, warn};
use zoomriya_keys::{DeviceKey, Purpose, SecretKey, seal, sign_request};
use zoomriya_proto::stun;
use zoomriya_proto::{
    ErrorBody, RELAY_TURN, TurnCredentials,
    laptop::{
        CHANNEL, Data, Frame, IceServer, LaptopAnswer, LaptopOffer, LaptopPairRequest,
        LaptopPaired, MAX_CHUNK, OFFER_CLOCK, RELAY_ANSWER, RELAY_WAIT, RELAY_WAIT_SECONDS,
        RelayedAnswer, RelayedOffer, RelayedOffers,
    },
};

use crate::{
    config::Target,
    devices::{Device, Devices},
    link::proxy_v2_header,
};

/// Most laptops connected at once.
const MAX_SESSIONS: usize = 32;
/// Bytes waiting for the data channel before we stop reading answers.
const MAX_QUEUED: usize = 4 << 20;
/// The data channel's "buffer low" mark.
const LOW_MARK: usize = 256 << 10;
/// Most requests open at once on one laptop's connection.
const MAX_REQUESTS: usize = 256;

/// What the home endpoint and the WebRTC loop share.
pub struct Laptops {
    /// Paired devices.
    pub devices: Arc<Devices>,
    /// The box key: seals answers and pairings.
    pub box_key: SecretKey,
    /// The box's name.
    pub fqdn: String,
    /// The ZOOMRIYA Connect server the box uses.
    pub server: String,
    /// The relays laptops reach the box through from away.
    pub relays: Vec<String>,
    /// STUN servers (`stun:host:port`).
    pub stun: Vec<String>,
    /// Box Desktop's address for laptops.
    pub desktop: String,
    /// The box's certificate bundle, whose key Firefox checks.
    pub bundle: std::path::PathBuf,
    /// For the relays' TURN credentials.
    http: reqwest::Client,
    /// TURN credentials from a relay, kept until half their life is gone.
    turn: tokio::sync::Mutex<Option<(TurnCredentials, u64)>>,
    /// New sessions, for the WebRTC loop.
    offers: mpsc::Sender<NewSession>,
}

/// What [`Laptops::start`] needs besides the box's keys and devices.
#[derive(Debug, Clone)]
pub struct Setup {
    /// The box's name.
    pub fqdn: String,
    /// The ZOOMRIYA Connect server the box uses.
    pub server: String,
    /// The web server laptops' pages come from.
    pub target: Target,
    /// WebRTC's UDP port.
    pub udp_port: u16,
    /// The box's own addresses, offered as candidates.
    pub hosts: Vec<IpAddr>,
    /// The relays, for laptops away from home.
    pub relays: Vec<String>,
    /// STUN servers, to learn the box's public address.
    pub stun: Vec<String>,
    /// Box Desktop's address for laptops.
    pub desktop: String,
    /// The box's certificate bundle.
    pub bundle: std::path::PathBuf,
    /// For the relays.
    pub http: reqwest::Client,
}

/// The home endpoint, on the home network only: HTTPS with the box's
/// certificate, or plain HTTP in development. Waits for the certificate.
pub async fn serve_home(l: Arc<Laptops>, port: u16, bundle: std::path::PathBuf, plain: bool) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let app = router(l);
    if plain {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                warn!(%addr, "laptops' home endpoint on plain HTTP: development only");
                let _ = zoomriya_serve::serve_plain(listener, app).await;
            }
            Err(e) => warn!("laptops' home endpoint: {e}"),
        }
        return;
    }
    loop {
        if bundle.exists() {
            let cert = zoomriya_serve::CertSource::Files {
                cert: bundle.clone(),
                key: bundle.clone(),
            };
            match zoomriya_serve::TlsListener::bind(addr, &cert, &[b"http/1.1"]).await {
                Ok(listener) => {
                    info!(%addr, "laptops' home endpoint");
                    let _ = zoomriya_serve::serve_https(listener, app).await;
                    return;
                }
                Err(e) => warn!("laptops' home endpoint: {e:#}"),
            }
        }
        // No certificate yet: the box gets one soon after its name.
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

struct NewSession {
    device: Device,
    offer: SdpOffer,
    reply: oneshot::Sender<Result<String>>,
}

impl Laptops {
    /// Binds WebRTC's port and returns what the home endpoint needs, and the
    /// loop to run.
    pub async fn start(
        devices: Arc<Devices>,
        box_key: SecretKey,
        setup: Setup,
    ) -> Result<(
        Arc<Self>,
        impl std::future::Future<Output = ()> + Send + 'static,
    )> {
        let Setup {
            fqdn,
            server,
            target,
            udp_port,
            hosts,
            relays,
            stun,
            desktop,
            bundle,
            http,
        } = setup;
        let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], udp_port)))
            .await
            .with_context(|| format!("WebRTC on UDP {udp_port}"))?;
        let port = socket.local_addr()?.port();
        let (tx, rx) = mpsc::channel(8);
        let forward = Forward {
            target,
            host: fqdn.clone(),
        };
        info!(port, "laptops: WebRTC listening");
        Ok((
            Arc::new(Self {
                devices,
                box_key,
                fqdn,
                server,
                relays,
                stun: stun.clone(),
                desktop,
                bundle,
                http,
                turn: tokio::sync::Mutex::new(None),
                offers: tx,
            }),
            run(socket, port, hosts, stun, rx, Arc::new(forward)),
        ))
    }

    /// Checks a laptop's offer and answers it.
    pub async fn connect(&self, offer: &LaptopOffer, now: u64) -> Result<String, Refusal> {
        if offer.time.abs_diff(now) > OFFER_CLOCK {
            return Err(Refusal::Clock);
        }
        let device = self
            .devices
            .by_link(&offer.link.0)
            .filter(|d| d.laptop)
            .ok_or(Refusal::Unknown)?;
        DeviceKey::from_b64(&device.device_key)
            .and_then(|k| k.verify_for(Purpose::LaptopOffer, &offer.signed_bytes(), &offer.sig))
            .map_err(|_| Refusal::Signature)?;
        let sdp = SdpOffer::from_sdp_string(&offer.sdp).map_err(|_| Refusal::Offer)?;
        let (reply, answer) = oneshot::channel();
        self.offers
            .send(NewSession {
                device: device.clone(),
                offer: sdp,
                reply,
            })
            .await
            .map_err(|_| Refusal::Busy)?;
        let answer_sdp = answer.await.map_err(|_| Refusal::Busy)?.map_err(|e| {
            warn!("laptop answer: {e:#}");
            Refusal::Offer
        })?;
        self.devices.seen(&device.id, now);
        // A new pass each time, if the clock can be trusted.
        let (pass, until) = match self.devices.fresh_pass(&device.id, &self.box_key, now) {
            Ok((p, u)) => (Some(p), u),
            Err(_) => (None, 0),
        };
        Ok(seal(
            &self.box_key,
            Purpose::LaptopAnswer,
            &LaptopAnswer {
                offer: BASE64URL_NOPAD.encode(&Sha256::digest(offer.sdp.as_bytes())),
                sdp: answer_sdp,
                pass,
                until,
                relays: self.relays.clone(),
                ice: self.ice().await,
                tls_keys: tls_keys(&self.bundle),
            },
        ))
    }

    /// The ICE servers laptops use: STUN, and TURN for when the direct path
    /// fails, with credentials from a relay (docs/laptops.md).
    pub async fn ice(&self) -> Vec<IceServer> {
        let mut out = Vec::new();
        if !self.stun.is_empty() {
            out.push(IceServer {
                urls: self.stun.clone(),
                username: None,
                credential: None,
            });
        }
        if let Some(t) = self.turn_credentials().await {
            out.push(IceServer {
                urls: t.urls,
                username: Some(t.username),
                credential: Some(t.credential),
            });
        }
        out
    }

    /// Credentials for the TURN server beside one of the relays: the ones
    /// kept, or new ones from the first relay that has a TURN server.
    async fn turn_credentials(&self) -> Option<TurnCredentials> {
        let mut kept = self.turn.lock().await;
        let now = crate::now();
        if let Some((c, until)) = kept.as_ref()
            && now + c.ttl / 2 < *until
        {
            return Some(c.clone());
        }
        for relay in &self.relays {
            match fetch_turn(&self.http, &self.box_key, relay).await {
                Ok(c) => {
                    *kept = Some((c.clone(), now + c.ttl));
                    return Some(c);
                }
                Err(e) => debug!(%relay, "TURN credentials: {e:#}"),
            }
        }
        // None new: the old ones while they last.
        kept.as_ref()
            .filter(|(_, until)| now < *until)
            .map(|(c, _)| c.clone())
    }
}

/// Asks a relay for credentials for the TURN server beside it.
async fn fetch_turn(
    http: &reqwest::Client,
    box_key: &SecretKey,
    relay: &str,
) -> Result<TurnCredentials> {
    let sig = sign_request(box_key, "POST", RELAY_TURN, crate::now(), b"");
    let res = http
        .post(format!("{}{RELAY_TURN}", relay.trim_end_matches('/')))
        .header(zoomriya_keys::REQUEST_SIGNATURE_HEADER, sig)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;
    anyhow::ensure!(res.status().is_success(), "relay said {}", res.status());
    Ok(res.json().await?)
}

/// The key of the box's certificate, as Firefox shows it for a page: SHA-256
/// of the leaf's SubjectPublicKeyInfo, base64. Empty without a certificate.
pub fn tls_keys(bundle: &std::path::Path) -> Vec<String> {
    let Ok(pem) = std::fs::read(bundle) else {
        return vec![];
    };
    let Ok((_, p)) = x509_parser::pem::parse_x509_pem(&pem) else {
        return vec![];
    };
    let Ok(cert) = p.parse_x509() else {
        return vec![];
    };
    vec![data_encoding::BASE64.encode(&Sha256::digest(cert.tbs_certificate.subject_pki.raw))]
}

/// Why a laptop was turned away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The laptop's clock is too far off.
    Clock,
    /// Not paired, or removed.
    Unknown,
    /// The device key's signature doesn't match.
    Signature,
    /// The offer can't be read or answered.
    Offer,
    /// Too many laptops at once, or connectd is stopping.
    Busy,
}

impl Refusal {
    fn response(self) -> Response {
        let (status, code, message) = self.parts();
        refuse(status, code, message)
    }

    fn body(self) -> ErrorBody {
        let (_, code, message) = self.parts();
        ErrorBody {
            code: code.into(),
            message: message.into(),
        }
    }

    fn parts(self) -> (StatusCode, &'static str, &'static str) {
        match self {
            Refusal::Clock => (
                StatusCode::BAD_REQUEST,
                "clock",
                "This computer's clock is wrong. Set it to the right time, then try again.",
            ),
            Refusal::Unknown => (
                StatusCode::FORBIDDEN,
                "unknown_device",
                "This laptop isn't paired with the box. Pair it again at home.",
            ),
            Refusal::Signature => (
                StatusCode::FORBIDDEN,
                "device_signature",
                "This laptop's key doesn't match. Pair it again at home.",
            ),
            Refusal::Offer => (
                StatusCode::BAD_REQUEST,
                "offer",
                "The connection couldn't be set up. Try again.",
            ),
            Refusal::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "busy",
                "The box is busy. Try again in a minute.",
            ),
        }
    }
}

fn refuse(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            code: code.into(),
            message: message.into(),
        }),
    )
        .into_response()
}

/// The home endpoint's routes. Only the home network may use them (the
/// caller checks the address too).
pub fn router(l: Arc<Laptops>) -> Router {
    Router::new()
        .route("/laptop/pair", post(pair))
        .route("/laptop/connect", post(connect))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .with_state(l)
}

fn from_home(addr: &SocketAddr) -> bool {
    zoomriya_proto::addrs::is_home(addr.ip()) || addr.ip().is_loopback()
}

async fn pair(
    State(l): State<Arc<Laptops>>,
    ConnectInfo(from): ConnectInfo<SocketAddr>,
    Json(req): Json<LaptopPairRequest>,
) -> Response {
    if !from_home(&from) {
        return refuse(
            StatusCode::FORBIDDEN,
            "not_home",
            "Pair laptops at home, on the box's network.",
        );
    }
    let now = crate::now();
    match l.devices.pair_laptop(&req, &l.box_key, now) {
        Ok(p) => {
            info!(device = %p.device.id, name = %p.device.name, "laptop paired");
            let sealed = seal(
                &l.box_key,
                Purpose::LaptopPaired,
                &LaptopPaired {
                    box_key: l.box_key.public().into(),
                    fqdn: l.fqdn.clone(),
                    device: p.device.id.clone(),
                    pass: p.pass,
                    until: p.device.pass_until,
                    server: l.server.clone(),
                    relays: l.relays.clone(),
                    ice: l.ice().await,
                    desktop: l.desktop.clone(),
                    tls_keys: tls_keys(&l.bundle),
                },
            );
            Json(serde_json::json!({ "paired": sealed })).into_response()
        }
        Err(why) => {
            let message = match why {
                crate::devices::Refused::PairingCode => {
                    "That code doesn't work. Check it, or show a new one in Box Desktop."
                }
                crate::devices::Refused::WrongLink => "This laptop is already paired.",
                other => other.message(),
            };
            refuse(StatusCode::FORBIDDEN, why.code(), message)
        }
    }
}

async fn connect(
    State(l): State<Arc<Laptops>>,
    ConnectInfo(from): ConnectInfo<SocketAddr>,
    Json(offer): Json<LaptopOffer>,
) -> Response {
    if !from_home(&from) {
        return refuse(
            StatusCode::FORBIDDEN,
            "not_home",
            "Away from home, laptops connect through the relay.",
        );
    }
    match l.connect(&offer, crate::now()).await {
        Ok(sealed) => Json(serde_json::json!({ "answer": sealed })).into_response(),
        Err(why) => why.response(),
    }
}

// --- From away: offers through the relays -----------------------------------

/// Waits on every relay for laptops' offers, while any laptop is paired.
pub async fn listen_on_relays(l: Arc<Laptops>, http: reqwest::Client) {
    // Each wait on a connection of its own: the relay ends API connections
    // after 30 seconds, so one kept from an earlier request could close in
    // the middle of a wait.
    let waits = match reqwest::Client::builder()
        .user_agent(concat!("zoomriya-connectd/", env!("CARGO_PKG_VERSION")))
        .pool_max_idle_per_host(0)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("laptops from away: {e}");
            return;
        }
    };
    let mut tasks = tokio::task::JoinSet::new();
    for relay in l.relays.clone() {
        tasks.spawn(listen_on(l.clone(), http.clone(), waits.clone(), relay));
    }
    while tasks.join_next().await.is_some() {}
}

async fn listen_on(l: Arc<Laptops>, http: reqwest::Client, waits: reqwest::Client, relay: String) {
    let base = relay.trim_end_matches('/').to_string();
    let mut backoff = 5;
    loop {
        let any = l
            .devices
            .all()
            .iter()
            .any(|d| d.laptop && d.removed_at.is_none());
        if !any {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        match wait_once(&l, &waits, &base).await {
            Ok(got) => {
                backoff = 5;
                for o in got.offers {
                    tokio::spawn(answer_relayed(l.clone(), http.clone(), base.clone(), o));
                }
            }
            Err(e) => {
                debug!(%relay, "laptops' offers: {e:#}");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(120);
            }
        }
    }
}

async fn wait_once(l: &Laptops, http: &reqwest::Client, base: &str) -> Result<RelayedOffers> {
    let sig = sign_request(&l.box_key, "POST", RELAY_WAIT, crate::now(), b"");
    let res = http
        .post(format!("{base}{RELAY_WAIT}"))
        .header(zoomriya_keys::REQUEST_SIGNATURE_HEADER, sig)
        .timeout(Duration::from_secs(RELAY_WAIT_SECONDS + 15))
        .send()
        .await?;
    anyhow::ensure!(res.status().is_success(), "relay said {}", res.status());
    Ok(res.json().await?)
}

async fn answer_relayed(l: Arc<Laptops>, http: reqwest::Client, base: String, o: RelayedOffer) {
    info!(relay = %base, "laptop offer through a relay");
    let reply = match l.connect(&o.offer, crate::now()).await {
        Ok(sealed) => RelayedAnswer {
            id: o.id,
            answer: Some(sealed),
            error: None,
        },
        Err(why) => RelayedAnswer {
            id: o.id,
            answer: None,
            error: Some(why.body()),
        },
    };
    let Ok(body) = serde_json::to_vec(&reply) else {
        return;
    };
    let sig = sign_request(&l.box_key, "POST", RELAY_ANSWER, crate::now(), &body);
    if let Err(e) = http
        .post(format!("{base}{RELAY_ANSWER}"))
        .header(zoomriya_keys::REQUEST_SIGNATURE_HEADER, sig)
        .header("content-type", "application/json")
        .timeout(Duration::from_secs(15))
        .body(body)
        .send()
        .await
    {
        debug!("laptop answer through the relay: {e}");
    }
}

// --- The WebRTC loop ---------------------------------------------------------

/// What the forwarding tasks tell the loop.
enum Up {
    Response {
        session: u64,
        id: u32,
        status: u16,
        headers: Vec<(String, String)>,
    },
    Chunk {
        session: u64,
        id: u32,
        bytes: Bytes,
    },
    End {
        session: u64,
        id: u32,
    },
    Error {
        session: u64,
        id: u32,
        message: String,
    },
}

struct Request {
    body: Option<mpsc::UnboundedSender<Bytes>>,
    task: tokio::task::AbortHandle,
}

struct Session {
    key: u64,
    rtc: Rtc,
    device: Device,
    channel: Option<ChannelId>,
    requests: HashMap<u32, Request>,
    queue: VecDeque<(bool, Vec<u8>)>,
    queued: usize,
    started: Instant,
    alive: bool,
}

impl Session {
    /// Writes a frame, or queues it while the channel's buffer is full.
    fn write(&mut self, binary: bool, bytes: Vec<u8>) {
        if self.queue.is_empty()
            && let Some(mut ch) = self.channel.and_then(|id| self.rtc.channel(id))
        {
            match ch.write(binary, &bytes) {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    debug!("data channel: {e}");
                    self.alive = false;
                    return;
                }
            }
        }
        self.queued += bytes.len();
        self.queue.push_back((binary, bytes));
    }

    /// Sends what was queued, as far as the channel takes it.
    fn flush(&mut self) {
        let Some(id) = self.channel else { return };
        while let Some((binary, bytes)) = self.queue.front() {
            let Some(mut ch) = self.rtc.channel(id) else {
                return;
            };
            match ch.write(*binary, bytes) {
                Ok(true) => {
                    let n = bytes.len();
                    self.queue.pop_front();
                    self.queued -= n;
                }
                Ok(false) => return,
                Err(_) => {
                    self.alive = false;
                    return;
                }
            }
        }
    }

    fn text(&mut self, frame: &Frame) {
        if let Ok(json) = serde_json::to_vec(frame) {
            self.write(false, json);
        }
    }
}

async fn run(
    socket: UdpSocket,
    port: u16,
    hosts: Vec<IpAddr>,
    stun_urls: Vec<String>,
    mut offers: mpsc::Receiver<NewSession>,
    forward: Arc<Forward>,
) {
    let socket = Arc::new(socket);
    let (up_tx, mut up_rx) = mpsc::channel::<Up>(256);
    let mut sessions: Vec<Session> = Vec::new();
    let mut next_key = 1u64;
    let mut buf = vec![0u8; 2048];
    // The box's public address on this port, from STUN.
    let mut stun_servers: Vec<SocketAddr> = Vec::new();
    let mut txid = [0u8; 12];
    let mut public: Option<SocketAddr> = None;
    let mut next_stun = Instant::now();
    loop {
        if !stun_urls.is_empty() && Instant::now() >= next_stun {
            stun_servers = resolve(&stun_urls).await;
            if getrandom::fill(&mut txid).is_ok() {
                for s in &stun_servers {
                    let _ = socket.send_to(&stun::request(&txid), s).await;
                }
            }
            // Again soon until it answers, then every few minutes, as
            // routers forget.
            let again = if public.is_some() { 120 } else { 15 };
            next_stun = Instant::now() + Duration::from_secs(again);
        }
        // Everything str0m wants done now.
        let mut wake = Instant::now() + Duration::from_secs(1);
        if !stun_urls.is_empty() {
            wake = wake.min(next_stun);
        }
        for s in sessions.iter_mut() {
            // What waited for room in the channel's buffer: sent as soon as
            // str0m takes it, not only when it says the buffer is low.
            if !s.queue.is_empty() {
                s.flush();
                if !s.queue.is_empty() {
                    wake = wake.min(Instant::now() + Duration::from_millis(5));
                }
            }
            loop {
                match s.rtc.poll_output() {
                    Ok(Output::Timeout(t)) => {
                        wake = wake.min(t);
                        break;
                    }
                    Ok(Output::Transmit(t)) => {
                        let _ = socket.send_to(&t.contents, t.destination).await;
                    }
                    Ok(Output::Event(e)) => event(s, e, &forward, &up_tx),
                    Err(e) => {
                        debug!("laptop {}: {e}", s.device.id);
                        s.alive = false;
                        break;
                    }
                }
            }
            // A connection that never came up in a minute goes.
            if s.channel.is_none() && s.started.elapsed() > Duration::from_secs(60) {
                s.alive = false;
            }
        }
        sessions.retain_mut(|s| {
            if !s.alive {
                info!(device = %s.device.id, "laptop disconnected");
                for r in s.requests.values() {
                    r.task.abort();
                }
            }
            s.alive
        });
        let queued: usize = sessions.iter().map(|s| s.queued).sum();

        let now = Instant::now();
        let sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(wake.max(now)));
        tokio::select! {
            got = socket.recv_from(&mut buf) => {
                let Ok((n, source)) = got else { continue };
                if stun_servers.contains(&source) {
                    if let Some(seen) = stun::mapped(&buf[..n], &txid)
                        && public != Some(seen)
                    {
                        info!(%seen, "laptops: the box's public address");
                        public = Some(seen);
                    }
                    continue;
                }
                let Ok(contents) = buf[..n].try_into() else { continue };
                let input = Input::Receive(Instant::now(), Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: SocketAddr::new(local_for(&hosts, source), port),
                    contents,
                });
                if let Some(s) = sessions.iter_mut().find(|s| s.rtc.accepts(&input))
                    && let Err(e) = s.rtc.handle_input(input) {
                        debug!("laptop {}: {e}", s.device.id);
                        s.alive = false;
                    }
            }
            Some(new) = offers.recv() => {
                if sessions.len() >= MAX_SESSIONS {
                    let _ = new.reply.send(Err(anyhow::anyhow!("too many laptops")));
                    continue;
                }
                match answer(new.offer, &hosts, port, public) {
                    Ok((rtc, sdp)) => {
                        info!(device = %new.device.id, name = %new.device.name, "laptop connecting");
                        sessions.push(Session {
                            key: next_key,
                            rtc,
                            device: new.device,
                            channel: None,
                            requests: HashMap::new(),
                            queue: VecDeque::new(),
                            queued: 0,
                            started: Instant::now(),
                            alive: true,
                        });
                        next_key += 1;
                        let _ = new.reply.send(Ok(sdp));
                    }
                    Err(e) => {
                        let _ = new.reply.send(Err(e));
                    }
                }
            }
            Some(up) = up_rx.recv(), if queued < MAX_QUEUED => {
                deliver(&mut sessions, up);
            }
            _ = sleep => {
                let now = Instant::now();
                for s in sessions.iter_mut() {
                    if s.rtc.handle_input(Input::Timeout(now)).is_err() {
                        s.alive = false;
                    }
                }
            }
        }
    }
}

/// The address a packet from `source` reached: the box's own address of the
/// same family, as str0m needs it to match a local candidate.
fn local_for(hosts: &[IpAddr], source: SocketAddr) -> IpAddr {
    if source.ip().is_loopback() {
        return source.ip();
    }
    hosts
        .iter()
        .find(|h| h.is_ipv4() == source.is_ipv4() && !h.is_loopback())
        .copied()
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// The STUN servers' addresses, IPv4: the port's public address is what
/// laptops away from home need.
async fn resolve(urls: &[String]) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for u in urls {
        let Some(hp) = stun::host_port(u) else {
            continue;
        };
        if let Ok(addrs) = tokio::net::lookup_host(hp).await {
            out.extend(addrs.filter(SocketAddr::is_ipv4));
        }
    }
    out
}

fn answer(
    offer: SdpOffer,
    hosts: &[IpAddr],
    port: u16,
    public: Option<SocketAddr>,
) -> Result<(Rtc, String)> {
    let mut rtc = RtcConfig::new()
        .set_crypto_provider(Arc::new(str0m::crypto::from_feature_flags()))
        .build(Instant::now());
    for ip in hosts {
        if ip.is_ipv4()
            && let Ok(c) = Candidate::host(SocketAddr::new(*ip, port), "udp")
        {
            rtc.add_local_candidate(c);
        }
    }
    // The router's side, for laptops away from home.
    if let Some(public) = public
        && let Some(base) = hosts.iter().find(|h| h.is_ipv4() && !h.is_loopback())
        && public.ip() != *base
        && let Ok(c) = Candidate::server_reflexive(public, SocketAddr::new(*base, port), "udp")
    {
        rtc.add_local_candidate(c);
    }
    let answer = rtc
        .sdp_api()
        .accept_offer(offer)
        .context("accepting the offer")?;
    Ok((rtc, answer.to_sdp_string()))
}

fn event(s: &mut Session, e: Event, forward: &Arc<Forward>, up: &mpsc::Sender<Up>) {
    match e {
        Event::IceConnectionStateChange(IceConnectionState::Disconnected) => s.alive = false,
        Event::ChannelOpen(id, label) => {
            if label == CHANNEL {
                s.channel = Some(id);
                if let Some(mut ch) = s.rtc.channel(id) {
                    ch.set_buffered_amount_low_threshold(LOW_MARK);
                }
                info!(device = %s.device.id, "laptop connected");
            }
        }
        Event::ChannelClose(id) if Some(id) == s.channel => s.alive = false,
        Event::ChannelBufferedAmountLow(_) => s.flush(),
        Event::ChannelData(d) if Some(d.id) == s.channel => {
            if d.binary {
                match Data::decode(&d.data) {
                    Some(Data::Chunk(id, bytes)) => {
                        if let Some(tx) = s.requests.get(&id).and_then(|r| r.body.as_ref()) {
                            let _ = tx.send(Bytes::copy_from_slice(bytes));
                        }
                    }
                    Some(Data::End(id)) => {
                        if let Some(r) = s.requests.get_mut(&id) {
                            r.body = None;
                        }
                    }
                    None => {}
                }
                return;
            }
            match serde_json::from_slice::<Frame>(&d.data) {
                Ok(Frame::Request {
                    id,
                    method,
                    path,
                    headers,
                }) => {
                    debug!(device = %s.device.id, id, %method, %path, "laptop request");
                    if s.requests.len() >= MAX_REQUESTS || s.requests.contains_key(&id) {
                        s.text(&Frame::Error {
                            id,
                            message: "Too many requests at once.".into(),
                        });
                        return;
                    }
                    let (tx, rx) = mpsc::unbounded_channel();
                    let task = tokio::spawn(forward.clone().request(
                        s.key,
                        id,
                        s.device.clone(),
                        method,
                        path,
                        headers,
                        rx,
                        up.clone(),
                    ));
                    s.requests.insert(
                        id,
                        Request {
                            body: Some(tx),
                            task: task.abort_handle(),
                        },
                    );
                }
                Ok(Frame::Cancel { id }) => {
                    if let Some(r) = s.requests.remove(&id) {
                        r.task.abort();
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn deliver(sessions: &mut [Session], up: Up) {
    let key = match &up {
        Up::Response { session, .. }
        | Up::Chunk { session, .. }
        | Up::End { session, .. }
        | Up::Error { session, .. } => *session,
    };
    let Some(s) = sessions.iter_mut().find(|s| s.key == key) else {
        return;
    };
    match up {
        Up::Response {
            id,
            status,
            headers,
            ..
        } => {
            debug!(device = %s.device.id, id, status, "laptop answer");
            s.text(&Frame::Response {
                id,
                status,
                headers,
            })
        }
        Up::Chunk { id, bytes, .. } => s.write(true, Data::Chunk(id, &bytes).encode()),
        Up::End { id, .. } => {
            debug!(device = %s.device.id, id, queued = s.queued, "laptop answer done");
            s.requests.remove(&id);
            s.write(true, Data::End(id).encode());
        }
        Up::Error { id, message, .. } => {
            s.requests.remove(&id);
            s.text(&Frame::Error { id, message });
        }
    }
}

// --- Forwarding to the box's web server --------------------------------------

/// Where laptops' requests go.
struct Forward {
    target: Target,
    /// The box's name, as the web server's `Host`.
    host: String,
}

/// Request headers never passed on from the laptop: the extension's own
/// address, and what the connection itself decides.
const DROPPED: &[&str] = &[
    "host",
    "origin",
    "referer",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
    "proxy-connection",
    "te",
];

impl Forward {
    #[allow(clippy::too_many_arguments)]
    async fn request(
        self: Arc<Self>,
        session: u64,
        id: u32,
        device: Device,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: mpsc::UnboundedReceiver<Bytes>,
        up: mpsc::Sender<Up>,
    ) {
        if let Err(e) = self
            .clone()
            .send(session, id, &device, method, path, headers, body, &up)
            .await
        {
            debug!("laptop {} request {id}: {e:#}", device.id);
            let _ = up
                .send(Up::Error {
                    session,
                    id,
                    message: "The box couldn't answer.".into(),
                })
                .await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn send(
        self: Arc<Self>,
        session: u64,
        id: u32,
        device: &Device,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: mpsc::UnboundedReceiver<Bytes>,
        up: &mpsc::Sender<Up>,
    ) -> Result<()> {
        anyhow::ensure!(path.starts_with('/'), "path must start with /");
        let scheme = if self.target.tls { "https" } else { "http" };
        let mut req = hyper::Request::builder()
            .method(method.as_str())
            .uri(path.as_str())
            .header(hyper::header::HOST, &self.host);
        for (k, v) in &headers {
            let k = k.to_ascii_lowercase();
            if DROPPED.contains(&k.as_str()) {
                continue;
            }
            req = req.header(k, v);
        }
        // What the box's own page sends: its own address.
        let own = format!("{scheme}://{}", self.host);
        if headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("origin"))
            || !matches!(method.as_str(), "GET" | "HEAD")
        {
            req = req.header(hyper::header::ORIGIN, &own);
        }
        req = req.header(hyper::header::REFERER, format!("{own}/"));
        let stream = tokio_stream_from(body);
        let body: BoxBody<Bytes, std::io::Error> = BodyExt::boxed(StreamBody::new(stream));
        let req = req.body(body)?;

        let header = if self.target.proxy_protocol {
            proxy_v2_header(device)
        } else {
            Vec::new()
        };
        let res = match (&self.target.unix, self.target.tcp) {
            (Some(path), _) => {
                let mut io = tokio::net::UnixStream::connect(path)
                    .await
                    .with_context(|| format!("reaching {}", path.display()))?;
                io.write_all(&header).await?;
                self.exchange(io, req).await?
            }
            (None, Some(addr)) => {
                let mut io = tokio::net::TcpStream::connect(addr)
                    .await
                    .with_context(|| format!("reaching {addr}"))?;
                io.write_all(&header).await?;
                self.exchange(io, req).await?
            }
            _ => anyhow::bail!("target has no address"),
        };

        let status = res.status().as_u16();
        let headers = res
            .headers()
            .iter()
            .filter(|(k, _)| {
                !matches!(
                    k.as_str(),
                    "connection" | "transfer-encoding" | "keep-alive"
                )
            })
            .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
            .collect();
        up.send(Up::Response {
            session,
            id,
            status,
            headers,
        })
        .await?;
        let mut body = res.into_body();
        while let Some(frame) = body.frame().await {
            let frame = frame?;
            if let Ok(data) = frame.into_data() {
                for part in data.chunks(MAX_CHUNK) {
                    up.send(Up::Chunk {
                        session,
                        id,
                        bytes: Bytes::copy_from_slice(part),
                    })
                    .await?;
                }
            }
        }
        up.send(Up::End { session, id }).await?;
        Ok(())
    }

    async fn exchange<S>(
        &self,
        io: S,
        req: hyper::Request<BoxBody<Bytes, std::io::Error>>,
    ) -> Result<hyper::Response<hyper::body::Incoming>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        if self.target.tls {
            let name = rustls::pki_types::ServerName::try_from(self.host.clone())?;
            let tls = tokio_rustls::TlsConnector::from(local_tls())
                .connect(name, io)
                .await
                .context("TLS to the box's web server")?;
            send_on(tls, req).await
        } else {
            send_on(io, req).await
        }
    }
}

async fn send_on<S>(
    io: S,
    req: hyper::Request<BoxBody<Bytes, std::io::Error>>,
) -> Result<hyper::Response<hyper::body::Incoming>>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(io)).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Ok(sender.send_request(req).await?)
}

fn tokio_stream_from(
    mut rx: mpsc::UnboundedReceiver<Bytes>,
) -> impl futures_lite::Stream<Item = Result<BodyFrame<Bytes>, std::io::Error>> + Send + 'static {
    futures_lite::stream::poll_fn(move |cx| {
        rx.poll_recv(cx).map(|b| b.map(|b| Ok(BodyFrame::data(b))))
    })
}

/// TLS to the box's own web server, on the box itself: the connection never
/// leaves the box (a loopback address or a socket only connectd can open),
/// so its certificate isn't checked against a certificate authority.
fn local_tls() -> Arc<rustls::ClientConfig> {
    static CONFIG: std::sync::OnceLock<Arc<rustls::ClientConfig>> = std::sync::OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()
                .map(|b| {
                    b.dangerous()
                        .with_custom_certificate_verifier(Arc::new(OnTheBox(provider)))
                        .with_no_client_auth()
                })
                .unwrap_or_else(|_| {
                    rustls::ClientConfig::builder()
                        .with_root_certificates(rustls::RootCertStore::empty())
                        .with_no_client_auth()
                });
            Arc::new(config)
        })
        .clone()
}

#[derive(Debug)]
struct OnTheBox(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for OnTheBox {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_certificate_key_as_firefox_shows_it() {
        let dir = tempfile::tempdir().expect("dir");
        let cert = rcgen::generate_simple_self_signed(vec!["maplehouse.zoomriya.app".into()])
            .expect("cert");
        let bundle = dir.path().join("connect.pem");
        std::fs::write(
            &bundle,
            format!("{}{}", cert.cert.pem(), cert.signing_key.serialize_pem()),
        )
        .expect("write");
        use rcgen::PublicKeyData;
        let spki = cert.signing_key.subject_public_key_info();
        assert_eq!(
            tls_keys(&bundle),
            vec![data_encoding::BASE64.encode(&Sha256::digest(&spki))]
        );
        assert!(tls_keys(&dir.path().join("none.pem")).is_empty());
    }
}
