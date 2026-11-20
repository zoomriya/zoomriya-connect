//! The control plane's HTTP API.
//!
//! | Route | Who | Signed with |
//! |---|---|---|
//! | `POST /v1/boxes` | A box registering | Box key |
//! | `PUT /v1/boxes/{name}/home` | The box: where its name points | DNS update key |
//! | `PUT /v1/boxes/{name}/acme` | The box: certificate-check records | DNS update key |
//! | `PUT`/`DELETE /v1/boxes/{name}/door` | The box: the guests' door | DNS update key |
//! | `POST /v1/boxes/{name}/release` | The box giving up its name | Box key |
//! | `PUT`/`GET /pkarr/{key}` | Boxes publish address records; apps read them | The record itself |
//! | `GET /v1/relay/boxes?after=N` | Relays | Control plane to relay key |
//! | `POST /v1/relay/report` | Relays: use per box, who's connected | Control plane to relay key |
//! | `POST /v1/boxes/{name}/relay-use` | The box: its use this month | Box key |
//! | `POST /v1/reports` | Anyone reporting a name | |
//! | `/v1/account/...`, `/account/` | Families: their ZOOMRIYA Connect account ([`crate::accounts`]) | Their sign-in |
//! | `GET /.well-known/zoomriya-connect.json` | Boxes setting up | |

// Handlers return a ready response as their error, the usual axum way; it's
// built once per refused request, so its size doesn't matter.
#![allow(clippy::result_large_err)]

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use iroh_dns::pkarr::SignedPacket;
use serde::Deserialize;
use subtle::ConstantTimeEq;
use tracing::info;
use zoomriya_keys::{PublicKey, REQUEST_SIGNATURE_HEADER, RequestSignature};
use zoomriya_proto::{
    AcmeRequest, DoorRequest, ErrorBody, HomeRequest, MIN_PROTOCOL_VERSION, RegisterRequest,
    RegisterResponse, VERSION_HEADER, WellKnown, addrs, names,
};

use crate::{
    limit::Limiter,
    store::{Refused, Rules, Store},
};

/// Largest address record: signature, time and a 1000-byte DNS packet.
const MAX_RECORD: usize = 64 + 8 + 1000;
/// Largest request body otherwise.
const MAX_BODY: usize = 16 * 1024;

/// What the API needs.
pub struct Api {
    /// Registrations.
    pub store: Arc<Store>,
    /// Registration rules.
    pub rules: Rules,
    /// The control plane to relay keys.
    pub relay_keys: Vec<String>,
    /// The well-known file's contents.
    pub well_known: WellKnown,
    /// Where box names live, such as `zoomriya.app`.
    pub zone: String,
    /// Clock allowance on signed requests, in seconds.
    pub clock_allowance: u64,
    /// The clock.
    pub now: fn() -> u64,
    /// Per address, for everything boxes send.
    pub per_addr: Limiter<IpAddr>,
    /// Per box key, for address records.
    pub per_record: Limiter<PublicKey>,
    /// Per address, for reports.
    pub per_report: Limiter<IpAddr>,
    /// Per box, for outside checks.
    pub per_box: Limiter<PublicKey>,
    /// Per box, for guests' relay credentials.
    pub per_box_turn: Limiter<PublicKey>,
    /// Per address, for guests' setup sockets.
    pub per_guest: Limiter<IpAddr>,
    /// Per address, for account sign-ins.
    pub per_sign_in: Limiter<IpAddr>,
    /// The families' account page's files.
    pub account_page: std::path::PathBuf,
    /// The guests' relay, if this server has one.
    pub turn: Option<crate::turn::Turn>,
    /// The outside check.
    pub checker: crate::check::DoorChecker,
    /// Who is connected for setup messages.
    pub hub: crate::signal::Hub,
    /// The guest page's own address, which guests' sockets must come from.
    pub share_origin: String,
    /// The second DNS server's keys.
    pub zone_keys: Vec<String>,
    /// What each relay last said is connected.
    pub seen: crate::usage::Seen,
}

/// The default limits.
pub struct Limits {
    /// See [`Api::per_addr`].
    pub per_addr: Limiter<IpAddr>,
    /// See [`Api::per_record`].
    pub per_record: Limiter<PublicKey>,
    /// See [`Api::per_report`].
    pub per_report: Limiter<IpAddr>,
    /// See [`Api::per_box`].
    pub per_box: Limiter<PublicKey>,
    /// See [`Api::per_box_turn`].
    pub per_box_turn: Limiter<PublicKey>,
    /// See [`Api::per_guest`].
    pub per_guest: Limiter<IpAddr>,
    /// See [`Api::per_sign_in`].
    pub per_sign_in: Limiter<IpAddr>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_addr: Limiter::new(5.0, 60),
            per_record: Limiter::per(Duration::from_secs(5), 10),
            per_report: Limiter::per(Duration::from_secs(360), 10),
            // Ten outside checks an hour per box.
            per_box: Limiter::per(Duration::from_secs(360), 10),
            // A credential per guest, sixty an hour, in bursts.
            per_box_turn: Limiter::per(Duration::from_secs(60), 30),
            // Guests' setup sockets: a few a minute per address.
            per_guest: Limiter::per(Duration::from_secs(10), 10),
            // Account sign-ins: one every 6 seconds per address, in bursts
            // of 20. Each account also locks after 5 wrong passwords.
            per_sign_in: Limiter::per(Duration::from_secs(6), 20),
        }
    }
}

type Shared = Arc<Api>;

pub(crate) fn refuse(r: Refused) -> Response {
    let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(ErrorBody {
            code: r.code.into(),
            message: r.message,
        }),
    )
        .into_response()
}

pub(crate) fn error(status: u16, code: &'static str, message: &str) -> Response {
    refuse(Refused {
        status,
        code,
        message: message.into(),
    })
}

pub(crate) fn slow_down() -> Response {
    error(
        429,
        "slow_down",
        "Too many requests. Try again in a minute.",
    )
}

/// Our routes.
pub fn router(api: Shared) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route(
            "/robots.txt",
            get(|| async { "User-agent: *\nDisallow: /\n" }),
        )
        .route("/.well-known/zoomriya-connect.json", get(well_known))
        .route("/v1/boxes", post(register))
        .route("/v1/boxes/{name}/home", put(home))
        .route("/v1/boxes/{name}/acme", put(acme))
        .route("/v1/boxes/{name}/door", put(door).delete(close_door))
        .route("/v1/boxes/{name}/release", post(release))
        .route("/v1/account/sign-in", post(crate::accounts::sign_in))
        .route("/v1/account", get(crate::accounts::me))
        .route(
            "/v1/account/password",
            post(crate::accounts::change_password),
        )
        .route("/v1/account/sign-out", post(crate::accounts::sign_out))
        .route("/v1/relay/boxes", get(feed))
        .route(
            "/v1/relay/report",
            post(crate::usage::report).layer(DefaultBodyLimit::max(64 << 20)),
        )
        .route("/v1/boxes/{name}/relay-use", post(crate::usage::box_use))
        .route("/v1/reports", post(report))
        .route("/pkarr/{key}", put(put_record).get(get_record))
        .route("/v1/boxes/{name}/check", post(crate::check::check))
        .route("/v1/boxes/{name}/turn", post(crate::turn::credentials))
        .route("/v1/admin/stolen", post(crate::notes::stolen))
        .route("/v1/admin/move", post(crate::notes::move_name))
        .route("/v1/signal/box", get(crate::signal::box_socket))
        .route("/v1/signal/guest/{name}", get(crate::signal::guest_socket))
        .route("/v1/zone", get(crate::zone::copy))
        .route("/v1/server/report", post(crate::zone::server_report))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .nest_service("/account", zoomriya_serve::admin::page(&api.account_page))
        .with_state(api)
}

async fn well_known(State(api): State<Shared>) -> Response {
    (
        [(header::CACHE_CONTROL, "max-age=3600")],
        Json(api.well_known.clone()),
    )
        .into_response()
}

/// Checks the protocol version and the signature; returns the signing key.
pub(crate) fn signed(
    api: &Api,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<PublicKey, Response> {
    let version = headers
        .get(VERSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok());
    match version {
        Some(v) if v >= MIN_PROTOCOL_VERSION => {}
        Some(_) => {
            return Err(error(
                426,
                "please_update",
                "Your box needs an update to use ZOOMRIYA Connect. It updates by itself overnight.",
            ));
        }
        None => {
            return Err(error(
                400,
                "no_version",
                "The request has no protocol version.",
            ));
        }
    }
    let sig = headers
        .get(REQUEST_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| RequestSignature::parse(v).ok())
        .ok_or_else(|| error(401, "unsigned", "The request isn't signed."))?;
    sig.verify(
        method.as_str(),
        uri.path(),
        body,
        (api.now)(),
        api.clock_allowance,
    )
    .map_err(|_| {
        error(
            401,
            "bad_signature",
            "The request's signature doesn't match. If your box's clock is wrong, check its internet connection.",
        )
    })?;
    Ok(sig.key)
}

pub(crate) fn json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, Response> {
    serde_json::from_slice(body)
        .map_err(|_| error(400, "bad_request", "The request can't be read."))
}

async fn register(
    State(api): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !api.per_addr.allow(&peer.ip()) {
        return slow_down();
    }
    let key = match signed(&api, &method, &uri, &headers, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    let req: RegisterRequest = match json(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if req.box_key.0 != key {
        return error(401, "bad_signature", "The request isn't signed by the box.");
    }
    let account = match &req.account {
        Some(login) => {
            if !api.per_sign_in.allow(&peer.ip()) {
                return slow_down();
            }
            match crate::accounts::check(&api, login).await {
                Ok((id, _)) => Some(id),
                Err(why) => return why.response(),
            }
        }
        None => None,
    };
    match api
        .store
        .register(&req, peer.ip(), (api.now)(), &api.rules, account)
    {
        Ok(r) => {
            if r.new {
                info!(name = %r.name, factory = r.factory, "registered");
            }
            let status = if r.new {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            (
                status,
                Json(RegisterResponse {
                    fqdn: format!("{}.{}", r.name, api.zone),
                    share_fqdn: format!("{}.{}", names::share_name(&r.name), api.zone),
                    name: r.name,
                    factory: r.factory,
                    account: r.account,
                }),
            )
                .into_response()
        }
        Err(e) => refuse(e),
    }
}

/// For the routes the DNS update key signs: checks it's that box's key.
pub(crate) fn by_dns_key(
    api: &Api,
    name: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
    peer: IpAddr,
) -> Result<String, Response> {
    if !api.per_addr.allow(&peer) {
        return Err(slow_down());
    }
    let key = signed(api, method, uri, headers, body)?;
    let name = names::normalise(name);
    match api.store.find(&name) {
        Ok(Some(row)) if row.dns_update_key == key && !row.disabled && !row.stolen => Ok(name),
        Ok(_) => Err(error(
            403,
            "not_your_box",
            "This box can't change that name.",
        )),
        Err(e) => Err(refuse(e)),
    }
}

async fn home(
    State(api): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let name = match by_dns_key(&api, &name, &method, &uri, &headers, &body, peer.ip()) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let req: HomeRequest = match json(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match api.store.set_home(&name, &req) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refuse(e),
    }
}

async fn acme(
    State(api): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let name = match by_dns_key(&api, &name, &method, &uri, &headers, &body, peer.ip()) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let req: AcmeRequest = match json(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match api
        .store
        .set_acme(&name, req.target, &req.values, (api.now)())
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refuse(e),
    }
}

async fn door(
    State(api): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let name = match by_dns_key(&api, &name, &method, &uri, &headers, &body, peer.ip()) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let req: DoorRequest = match json(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if req.ipv4.is_none() && req.ipv6.is_none() {
        return error(400, "bad_request", "Say where the door is, or close it.");
    }
    // Each address must be public and the home's own: the one this request
    // came from, or in its IPv6 /56. The box sends one request over IPv4 and
    // one over IPv6.
    let from = peer.ip();
    let ok = |a: IpAddr| addrs::is_public(a) && addrs::same_home(a, from);
    if req.ipv4.is_some_and(|a| !ok(a.into())) || req.ipv6.is_some_and(|a| !ok(a.into())) {
        return error(
            403,
            "door_not_home",
            "The guests' door can only point at your own home's internet address.",
        );
    }
    // Keep the other family's address while it's still live.
    let now = (api.now)();
    let current = api
        .store
        .entry(&name)
        .filter(|e| e.door_until > now)
        .unwrap_or_default();
    let v4 = req.ipv4.or(current.door_v4);
    let v6 = req.ipv6.or(current.door_v6);
    match api.store.set_door(&name, v4, v6, now) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refuse(e),
    }
}

async fn close_door(
    State(api): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let name = match by_dns_key(&api, &name, &method, &uri, &headers, &body, peer.ip()) {
        Ok(n) => n,
        Err(r) => return r,
    };
    match api.store.set_door(&name, None, None, (api.now)()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refuse(e),
    }
}

async fn release(
    State(api): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !api.per_addr.allow(&peer.ip()) {
        return slow_down();
    }
    let key = match signed(&api, &method, &uri, &headers, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    let name = names::normalise(&name);
    match api.store.find(&name) {
        Ok(Some(row)) if row.box_key == key && !row.stolen => {}
        Ok(_) => return error(403, "not_your_box", "This box can't give up that name."),
        Err(e) => return refuse(e),
    }
    match api
        .store
        .release(&name, (api.now)(), api.rules.released_name_days)
    {
        Ok(()) => {
            info!(%name, "released");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => refuse(e),
    }
}

#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: u64,
}

/// Whether the request carries one of our control plane to relay keys.
pub(crate) fn is_relay(api: &Api, headers: &HeaderMap) -> bool {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    api.relay_keys.iter().fold(false, |hit, k| {
        hit | bool::from(k.as_bytes().ct_eq(token.as_bytes()))
    })
}

async fn feed(State(api): State<Shared>, headers: HeaderMap, Query(q): Query<After>) -> Response {
    if !is_relay(&api, &headers) {
        return error(401, "not_a_relay", "Only our relays can ask for this.");
    }
    match api.store.feed(q.after) {
        Ok(f) => Json(f).into_response(),
        Err(e) => refuse(e),
    }
}

#[derive(Deserialize)]
struct Report {
    name: String,
    reason: String,
}

async fn report(
    State(api): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(r): Json<Report>,
) -> Response {
    if !api.per_report.allow(&peer.ip()) {
        return slow_down();
    }
    let name = names::normalise(&r.name);
    let name = name
        .strip_suffix(&format!(".{}", api.zone))
        .unwrap_or(&name)
        .to_string();
    if name.is_empty() || name.len() > 80 || r.reason.trim().is_empty() || r.reason.len() > 2000 {
        return error(
            400,
            "bad_request",
            "Say which name, and what's wrong with it.",
        );
    }
    match api
        .store
        .report(&name, r.reason.trim(), peer.ip(), (api.now)())
    {
        Ok(()) => {
            info!(%name, "name reported");
            (StatusCode::ACCEPTED, "Thank you. We'll look at it.\n").into_response()
        }
        Err(e) => refuse(e),
    }
}

async fn put_record(State(api): State<Shared>, Path(key): Path<String>, body: Bytes) -> Response {
    let Ok(key) = PublicKey::from_z32(&key) else {
        return error(400, "bad_key", "That isn't a box key.");
    };
    if body.len() > MAX_RECORD {
        return error(413, "too_big", "The record is too big.");
    }
    if !api.per_record.allow(&key) {
        return slow_down();
    }
    let Ok(packet) = SignedPacket::from_relay_payload(&key, &body) else {
        return error(400, "bad_record", "The record isn't signed by that key.");
    };
    if let Some(old) = api.store.record(&key)
        && let Ok(old) = SignedPacket::from_relay_payload(&key, &old)
        && !packet.more_recent_than(&old)
    {
        return error(409, "older_record", "A newer record is already published.");
    }
    match api.store.put_record(&key, &body) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refuse(e),
    }
}

async fn get_record(State(api): State<Shared>, Path(key): Path<String>) -> Response {
    let Ok(key) = PublicKey::from_z32(&key) else {
        return error(400, "bad_key", "That isn't a box key.");
    };
    match api.store.record(&key) {
        Some(payload) => (
            [
                (header::CONTENT_TYPE, "application/pkarr.org/relays#payload"),
                (header::CACHE_CONTROL, "max-age=30"),
            ],
            payload,
        )
            .into_response(),
        None => error(404, "not_found", "No record for that box."),
    }
}
