//! The control plane's admin API, behind the GUI. Each action is one of the
//! operator commands, and is logged with the admin's name.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zoomriya_serve::admin::AdminName;

use crate::{
    accounts,
    api::Api,
    store::{Allowances, GB},
};

/// How a relay answered the last time we asked.
#[derive(Debug, Clone, Serialize)]
pub struct RelayHealth {
    /// Its URL.
    pub url: String,
    /// Whether `/healthz` answered.
    pub up: bool,
    /// How long it took, in milliseconds.
    pub ms: Option<u64>,
    /// Seconds since 1970 when we asked.
    pub checked: u64,
}

/// What the admin API needs.
pub struct Admin {
    /// The public API's state.
    pub api: Arc<Api>,
    /// The relays' health.
    pub relays: Arc<Mutex<Vec<RelayHealth>>>,
}

type Shared = Arc<Admin>;

/// The routes, mounted under `/admin/api`.
pub fn router(admin: Shared) -> Router {
    Router::new()
        .route("/overview", get(overview))
        .route("/boxes", get(boxes))
        .route("/boxes/{name}/{action}", post(box_action))
        .route("/reports", get(reports))
        .route("/reports/{id}/done", post(report_done))
        .route("/factory-tokens", post(factory_tokens))
        .route("/allowances", get(allowances).put(set_allowances))
        .route("/boxes/{name}/allowance", post(box_allowance))
        .route("/connected", get(connected))
        .route("/servers", get(servers))
        .route("/servers/{name}/forget", post(forget_server))
        .route("/accounts", get(accounts).post(new_account))
        .route("/accounts/{username}/{action}", post(account_action))
        .with_state(admin)
}

fn failed(err: impl std::fmt::Display) -> Response {
    warn!("admin: {err}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Something went wrong. Look at the server's log.\n",
    )
        .into_response()
}

async fn overview(State(a): State<Shared>) -> Response {
    let s = &a.api.store;
    let now = (a.api.now)();
    let counts = s.counts();
    let today = s.registered_since(now.saturating_sub(86_400));
    let week = s.registered_since(now.saturating_sub(7 * 86_400));
    let reports = s.open_reports();
    let tokens = s.factory_token_counts();
    let (Ok((all, factory, stolen, off)), Ok(today), Ok(week), Ok(reports), Ok((unused, used))) =
        (counts, today, week, reports, tokens)
    else {
        return failed("reading the database");
    };
    let (boxes_online, guests_online) = a.api.hub.counts();
    let now_connected = a.api.seen.connected(now);
    let month = zoomriya_proto::month_of(now);
    let (Ok((month_relay, month_turn)), Ok(allowances), Ok((accounts_all, accounts_blocked))) = (
        s.relay_use_total(&month),
        s.allowances(),
        s.account_counts(),
    ) else {
        return failed("reading the database");
    };
    let relays = a.relays.lock().map(|r| r.clone()).unwrap_or_default();
    let servers = s.servers(&month).unwrap_or_default();
    let servers_online = servers
        .iter()
        .filter(|r| now.saturating_sub(r.last_seen) <= ONLINE_FOR)
        .count();
    Json(serde_json::json!({
        "servers": { "online": servers_online, "all": servers.len() },
        "kind": "control_plane",
        "version": env!("CARGO_PKG_VERSION"),
        "zone": a.api.zone,
        "boxes": { "all": all, "factory": factory, "stolen": stolen, "off": off },
        "new": { "day": today, "week": week },
        "reports_open": reports.len(),
        "factory_tokens": { "unused": unused, "used": used },
        "setup_messages": { "boxes": boxes_online, "guests": guests_online },
        "guest_relay": a.api.turn.is_some(),
        "relays": relays,
        "connected": {
            "boxes": now_connected.len(),
            "devices": now_connected.iter().map(|c| u64::from(c.2)).sum::<u64>(),
        },
        "relay_use": { "month": month, "bytes": month_relay, "turn": month_turn },
        "allowances": allowances,
        "accounts": { "all": accounts_all, "blocked": accounts_blocked },
    }))
    .into_response()
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}

/// One box in the GUI's list, with its allowance and where it's connected.
#[derive(Serialize)]
struct BoxLine {
    #[serde(flatten)]
    summary: crate::store::BoxSummary,
    /// This month's allowance in GB (0: no limit).
    allowance_gb: u32,
    /// Past it: slowed.
    slowed: bool,
    /// The relay it's connected to now, and how many of its devices are.
    relay: Option<String>,
    devices: u32,
}

async fn boxes(State(a): State<Shared>, Query(q): Query<Search>) -> Response {
    let s = &a.api.store;
    let (Ok(list), Ok(al)) = (s.list_boxes(q.q.trim(), 200), s.allowances()) else {
        return failed("reading the database");
    };
    let live: HashMap<String, (String, u32)> = a
        .api
        .seen
        .connected((a.api.now)())
        .into_iter()
        .map(|(relay, key, devices)| (key.to_z32(), (relay, devices)))
        .collect();
    let lines: Vec<BoxLine> = list
        .into_iter()
        .map(|b| {
            let allowance_gb = b.own_allowance_gb.unwrap_or(if b.factory {
                al.factory_gb
            } else {
                al.default_gb
            });
            let slowed =
                allowance_gb > 0 && b.used.saturating_add(b.turn) > u64::from(allowance_gb) * GB;
            let (relay, devices) = live
                .get(&b.key)
                .map_or((None, 0), |(r, d)| (Some(r.clone()), *d));
            BoxLine {
                summary: b,
                allowance_gb,
                slowed,
                relay,
                devices,
            }
        })
        .collect();
    Json(lines).into_response()
}

/// A server counts as online if it reported within this long.
const ONLINE_FOR: u64 = 120;

/// Every server: how it's doing, and what it moved this month.
async fn servers(State(a): State<Shared>) -> Response {
    let now = (a.api.now)();
    let month = zoomriya_proto::month_of(now);
    let Ok(rows) = a.api.store.servers(&month) else {
        return failed("reading the database");
    };
    let mut totals = [0u64; 4];
    let list: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            for (t, v) in totals.iter_mut().zip([r.rx, r.tx, r.relay, r.turn]) {
                *t = t.saturating_add(v);
            }
            serde_json::json!({
                "name": r.name,
                "online": now.saturating_sub(r.last_seen) <= ONLINE_FOR,
                "last_seen": r.last_seen,
                "stats": r.stats,
                "month": { "rx": r.rx, "tx": r.tx, "relay": r.relay, "turn": r.turn },
            })
        })
        .collect();
    Json(serde_json::json!({
        "now": now,
        "month": month,
        "servers": list,
        "totals": { "rx": totals[0], "tx": totals[1], "relay": totals[2], "turn": totals[3] },
    }))
    .into_response()
}

/// Takes a server off the Servers page, for one shut down for good. It
/// comes back if it reports again.
async fn forget_server(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Path(name): Path<String>,
) -> Response {
    match a.api.store.forget_server(&name) {
        Ok(true) => {
            tracing::info!(admin = %who, server = %name, "server forgotten");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "No such server.\n").into_response(),
        Err(e) => failed(e),
    }
}

/// Every box connected to a relay now.
async fn connected(State(a): State<Shared>) -> Response {
    let live = a.api.seen.connected((a.api.now)());
    let keys: Vec<_> = live.iter().map(|c| c.1).collect();
    let Ok(names) = a.api.store.names_for(&keys) else {
        return failed("reading the database");
    };
    let mut out: Vec<serde_json::Value> = live
        .into_iter()
        .map(|(relay, key, devices)| {
            serde_json::json!({
                "name": names.get(&key),
                "key": key.to_z32(),
                "relay": relay,
                "devices": devices,
            })
        })
        .collect();
    out.sort_by(|x, y| x["name"].as_str().cmp(&y["name"].as_str()));
    Json(serde_json::json!({ "boxes": out, "relays": a.api.seen.relays() })).into_response()
}

async fn allowances(State(a): State<Shared>) -> Response {
    match a.api.store.allowances() {
        Ok(al) => Json(al).into_response(),
        Err(e) => failed(e),
    }
}

async fn set_allowances(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Json(al): Json<Allowances>,
) -> Response {
    if al.slow_kbps == 0 || al.default_gb > 100_000 || al.factory_gb > 100_000 {
        return (StatusCode::BAD_REQUEST, "Those numbers don't look right.\n").into_response();
    }
    match a.api.store.set_allowances(&al) {
        Ok(()) => {
            info!(admin = %who, ?al, "allowances changed");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => failed(e),
    }
}

#[derive(Deserialize)]
struct OneAllowance {
    /// GB a month (0: no limit), or nothing for the usual allowance.
    gb: Option<u32>,
}

async fn box_allowance(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Path(name): Path<String>,
    Json(o): Json<OneAllowance>,
) -> Response {
    if o.gb.is_some_and(|g| g > 100_000) {
        return (StatusCode::BAD_REQUEST, "That's more than 100 TB.\n").into_response();
    }
    match a.api.store.set_box_allowance(&name, o.gb) {
        Ok(true) => {
            info!(admin = %who, %name, gb = ?o.gb, "box allowance");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "No box has that name.\n").into_response(),
        Err(e) => failed(e),
    }
}

async fn box_action(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Path((name, action)): Path<(String, String)>,
) -> Response {
    let s = &a.api.store;
    let done = match action.as_str() {
        "switch-off" => s.set_disabled(&name, true),
        "switch-on" => s.set_disabled(&name, false),
        "mark-stolen" => s.set_stolen(&name),
        _ => return (StatusCode::NOT_FOUND, "No such action.\n").into_response(),
    };
    match done {
        Ok(true) => {
            info!(admin = %who, %name, %action, "admin action");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "No box has that name.\n").into_response(),
        Err(e) => failed(e),
    }
}

#[derive(Serialize)]
struct Report {
    id: i64,
    name: String,
    reason: String,
    at: u64,
}

async fn reports(State(a): State<Shared>) -> Response {
    match a.api.store.open_reports() {
        Ok(r) => Json(
            r.into_iter()
                .map(|(id, name, reason, at)| Report {
                    id,
                    name,
                    reason,
                    at,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => failed(e),
    }
}

async fn report_done(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Path(id): Path<i64>,
) -> Response {
    match a.api.store.close_report(id) {
        Ok(true) => {
            info!(admin = %who, id, "report closed");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "No such report.\n").into_response(),
        Err(e) => failed(e),
    }
}

#[derive(Deserialize)]
struct Tokens {
    tokens: String,
}

async fn factory_tokens(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Json(t): Json<Tokens>,
) -> Response {
    match a.api.store.import_factory_tokens(t.tokens.lines()) {
        Ok(n) => {
            info!(admin = %who, n, "factory tokens added");
            Json(serde_json::json!({ "added": n })).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response(),
    }
}

#[derive(Deserialize)]
struct NewAccount {
    username: String,
    #[serde(default)]
    max_boxes: Option<u32>,
}

async fn accounts(State(a): State<Shared>, Query(q): Query<Search>) -> Response {
    match a.api.store.list_accounts(q.q.trim(), 500) {
        Ok(list) => Json(list).into_response(),
        Err(e) => failed(e),
    }
}

/// Makes an account; answers with its password, for showing once.
async fn new_account(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Json(n): Json<NewAccount>,
) -> Response {
    let username = accounts::normalise(&n.username);
    if !accounts::valid_username(&username) {
        return (
            StatusCode::BAD_REQUEST,
            "Usernames are 3 to 64 letters, digits and . - _ @ +\n",
        )
            .into_response();
    }
    let max = n
        .max_boxes
        .unwrap_or(accounts::DEFAULT_MAX_BOXES)
        .clamp(1, 100);
    let made = async {
        let password = accounts::new_password()?;
        let p = password.clone();
        let h = tokio::task::spawn_blocking(move || accounts::hash(&p)).await??;
        let made = a
            .api
            .store
            .create_account(&username, &h, max, (a.api.now)())?;
        anyhow::Ok(made.then_some(password))
    };
    match made.await {
        Ok(Some(password)) => {
            info!(admin = %who, %username, max, "account made");
            Json(
                serde_json::json!({ "username": username, "password": password, "max_boxes": max }),
            )
            .into_response()
        }
        Ok(None) => (
            StatusCode::CONFLICT,
            "There's already an account with that username.\n",
        )
            .into_response(),
        Err(e) => failed(e),
    }
}

#[derive(Deserialize, Default)]
struct AccountChange {
    #[serde(default)]
    max_boxes: Option<u32>,
}

/// Resets the password, blocks, unblocks, or changes the most boxes.
async fn account_action(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
    Path((username, action)): Path<(String, String)>,
    body: Option<Json<AccountChange>>,
) -> Response {
    let username = accounts::normalise(&username);
    let s = &a.api.store;
    let not_found = || (StatusCode::NOT_FOUND, "No account has that username.\n").into_response();
    let done = match action.as_str() {
        "reset-password" => {
            let made = async {
                let password = accounts::new_password()?;
                let p = password.clone();
                let h = tokio::task::spawn_blocking(move || accounts::hash(&p)).await??;
                anyhow::Ok(
                    s.set_password(&username, &h, (a.api.now)())?
                        .then_some(password),
                )
            };
            return match made.await {
                Ok(Some(password)) => {
                    info!(admin = %who, %username, "account password reset");
                    Json(serde_json::json!({ "username": username, "password": password }))
                        .into_response()
                }
                Ok(None) => not_found(),
                Err(e) => failed(e),
            };
        }
        "block" => s.set_account_blocked(&username, true),
        "unblock" => s.set_account_blocked(&username, false),
        "max-boxes" => {
            let Some(max) = body
                .and_then(|b| b.0.max_boxes)
                .filter(|m| (1..=100).contains(m))
            else {
                return (StatusCode::BAD_REQUEST, "1 to 100 boxes.\n").into_response();
            };
            s.set_max_boxes(&username, max)
        }
        _ => return (StatusCode::NOT_FOUND, "No such action.\n").into_response(),
    };
    match done {
        Ok(true) => {
            info!(admin = %who, %username, %action, "account action");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => not_found(),
        Err(e) => failed(e),
    }
}

/// Asks each relay's `/healthz` every 30 seconds.
pub async fn watch_relays(urls: Vec<String>, out: Arc<Mutex<Vec<RelayHealth>>>) {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    else {
        return;
    };
    loop {
        let mut all = Vec::new();
        for url in &urls {
            let start = Instant::now();
            let ok = client
                .get(format!("{}/healthz", url.trim_end_matches('/')))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            all.push(RelayHealth {
                url: url.clone(),
                up: ok,
                ms: ok.then(|| start.elapsed().as_millis() as u64),
                checked: zoomriya_serve::now(),
            });
        }
        if let Ok(mut o) = out.lock() {
            *o = all;
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
