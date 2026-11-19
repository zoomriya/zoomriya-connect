//! The control plane's admin API, behind the GUI. Each action is one of the
//! operator commands, and is logged with the admin's name.

use std::{
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

use crate::api::Api;

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
    let relays = a.relays.lock().map(|r| r.clone()).unwrap_or_default();
    Json(serde_json::json!({
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
    }))
    .into_response()
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}

async fn boxes(State(a): State<Shared>, Query(s): Query<Search>) -> Response {
    match a.api.store.list_boxes(s.q.trim(), 200) {
        Ok(list) => Json(list).into_response(),
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
