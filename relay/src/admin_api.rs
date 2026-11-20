//! The relay's admin API, behind the GUI: how it's doing (the machine and
//! its TURN server too), and for a family's own relay, the one-time code that
//! introduces a box.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tracing::info;
use zoomriya_serve::admin::AdminName;

use crate::{enrol, gate::Gate, turn::Turn};

/// What the admin API needs.
pub struct Admin {
    /// The gate.
    pub gate: Arc<Gate>,
    /// Set for a family's own relay: where codes live.
    pub enrol_dir: Option<PathBuf>,
    /// The state directory, whose disk the GUI shows.
    pub state_dir: PathBuf,
    /// When the relay started.
    pub started: u64,
    /// The TURN server beside it.
    pub turn: Option<Arc<Turn>>,
    /// Its name, if it has one in a control plane.
    pub name: String,
}

type Shared = Arc<Admin>;

/// The routes, mounted under `/admin/api`.
pub fn router(admin: Shared) -> Router {
    Router::new()
        .route("/overview", get(overview))
        .route("/enrol-code", post(enrol_code))
        .with_state(admin)
}

async fn overview(State(a): State<Shared>) -> Response {
    let m = &a.gate.metrics;
    let (boxes_live, devices_live) = a.gate.live();
    let refused = m.refused_missing.get()
        + m.refused_malformed.get()
        + m.refused_bad_signature.get()
        + m.refused_wrong_device.get()
        + m.refused_not_yet_valid.get()
        + m.refused_expired.get()
        + m.refused_too_long.get()
        + m.refused_unknown_box.get()
        + m.refused_removed.get()
        + m.refused_stolen.get();
    let mut server = zoomriya_serve::host::stats(
        &a.name,
        "relay",
        env!("CARGO_PKG_VERSION"),
        a.started,
        &a.state_dir,
    );
    server.turn = a.turn.as_ref().map(|t| t.up());
    Json(serde_json::json!({
        "kind": "relay",
        "now": zoomriya_serve::now(),
        "server": server,
        "version": env!("CARGO_PKG_VERSION"),
        "mode": if a.enrol_dir.is_some() { "enrolled" } else { "control_plane" },
        "boxes": a.gate.boxes().len(),
        "connected": { "boxes": boxes_live, "devices": devices_live },
        "connected_boxes": a.gate.connected_boxes().into_iter()
            .map(|(k, devices)| serde_json::json!({ "key": k.to_z32(), "devices": devices }))
            .collect::<Vec<_>>(),
        "slowed": a.gate.meter.slowed(),
        "since_start": {
            "boxes_allowed": m.boxes_allowed.get(),
            "devices_allowed": m.devices_allowed.get(),
            "refused": refused,
            "refused_removed": m.refused_removed.get(),
            "refused_expired": m.refused_expired.get(),
            "dropped": m.dropped.get(),
            "turn": a.turn.as_ref().map_or(0, |t| t.total()),
        },
    }))
    .into_response()
}

async fn enrol_code(
    State(a): State<Shared>,
    Extension(AdminName(who)): Extension<AdminName>,
) -> Response {
    let Some(dir) = &a.enrol_dir else {
        return (
            StatusCode::NOT_FOUND,
            "This relay gets its boxes from its control plane.\n",
        )
            .into_response();
    };
    match enrol::new_code(dir, zoomriya_serve::now()) {
        Ok(code) => {
            info!(admin = %who, "enrol code made");
            Json(serde_json::json!({ "code": code, "minutes": enrol::CODE_LIFETIME / 60 }))
                .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}\n")).into_response(),
    }
}
