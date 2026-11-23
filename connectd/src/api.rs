//! Box Desktop's way into connectd: a Unix socket that only box-desktopd's
//! group can open. Box Desktop's own sign-in decides who may use it (admins).
//! Nothing here needs ZOOMRIYA's servers or the internet.
//!
//! | Route | What |
//! |---|---|
//! | `GET /v1/status` | The box key, how many devices are paired and connected, whether a code is shown, how the box is reached from away (`reach`), whether its relay runs behind Tor (`tor`), and whether remote access away from home is on (`away`) |
//! | `GET /v1/devices` | Paired devices, with how each connects |
//! | `POST /v1/pairing` `{name, admin, laptop}` | A pairing code for the QR code; for a laptop, a short code to type into the laptop app |
//! | `DELETE /v1/pairing` | Stops pairing |
//! | `DELETE /v1/devices/{id}` | Removes a device, and cuts its connections at once |
//! | `POST /v1/devices/{id}/name` `{name}` | Renames a device |
//! | `PUT /v1/away` `{on}` | Turns remote access away from home on or off (crate::away) |

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use tracing::info;
use zoomriya_proto::{ErrorBody, link::PairingCode};

use crate::{
    daemon::Ctx,
    devices::{Device, PAIRING_LIFETIME},
    net,
};

type Shared = Arc<Ctx>;

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            code: code.into(),
            message: message.into(),
        }),
    )
        .into_response()
}

fn failed(e: anyhow::Error) -> Response {
    tracing::warn!("api: {e:#}");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "Something went wrong on the box. Try again.",
    )
}

/// The routes.
pub fn router(ctx: Shared) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/devices", get(devices))
        .route("/v1/devices/{id}", delete(remove))
        .route("/v1/devices/{id}/name", post(rename))
        .route("/v1/pairing", post(start_pairing).delete(stop_pairing))
        .route("/v1/away", put(set_away))
        .with_state(ctx)
}

/// Serves the API on the Unix socket, readable and writable by its group.
pub async fn serve(socket: PathBuf, ctx: Shared) -> Result<()> {
    let socket = socket.as_path();
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    let _ = std::fs::remove_file(socket);
    let listener = tokio::net::UnixListener::bind(socket)
        .with_context(|| format!("listening on {}", socket.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660))?;
    }
    info!("Box Desktop's API on {}", socket.display());
    axum::serve(listener, router(ctx)).await?;
    Ok(())
}

#[derive(Serialize)]
struct Status {
    box_key: String,
    devices: usize,
    connected: usize,
    pairing: bool,
    reach: crate::reach::Reach,
    /// Whether the box's relay runs behind its onion service.
    tor: bool,
    /// Whether paired devices may reach the box away from home.
    away: bool,
    /// The box's UDP port, for the router steps.
    port: u16,
}

async fn status(State(ctx): State<Shared>) -> Response {
    let connected = ctx
        .live
        .lock()
        .ok()
        .and_then(|l| l.as_ref().map(|l| l.count()))
        .unwrap_or(0);
    Json(Status {
        box_key: ctx.box_key.public().to_z32(),
        devices: ctx.devices.all().len(),
        connected,
        pairing: ctx.devices.pairing_open(crate::now())
            || ctx.devices.laptop_pairing_open(crate::now()),
        reach: ctx.reach.lock().map(|r| r.clone()).unwrap_or_default(),
        tor: ctx.onion.lock().is_ok_and(|o| o.is_some()),
        away: ctx.away.on(),
        port: ctx.cfg.port,
    })
    .into_response()
}

#[derive(Serialize)]
struct DeviceView {
    #[serde(flatten)]
    device: Device,
    connected: bool,
}

async fn devices(State(ctx): State<Shared>) -> Response {
    let live = ctx.live.lock().ok().and_then(|l| l.clone());
    let list: Vec<DeviceView> = ctx
        .devices
        .all()
        .into_iter()
        .map(|d| DeviceView {
            connected: live.as_ref().is_some_and(|l| l.is_connected(&d.link.0)),
            device: d,
        })
        .collect();
    Json(list).into_response()
}

#[derive(Deserialize)]
struct NewPairing {
    #[serde(default)]
    name: String,
    #[serde(default)]
    admin: bool,
    /// A laptop: a short code to type into the laptop app instead of a QR code.
    #[serde(default)]
    laptop: bool,
}

async fn start_pairing(State(ctx): State<Shared>, Json(req): Json<NewPairing>) -> Response {
    let now = crate::now();
    if req.laptop {
        return match ctx.devices.start_laptop_pairing(&req.name, now) {
            Ok(code) => Json(serde_json::json!({
                "code": code,
                "expires_in": PAIRING_LIFETIME,
            }))
            .into_response(),
            Err(e) => failed(e),
        };
    }
    let secret = match ctx.devices.start_pairing(&req.name, req.admin, now) {
        Ok(s) => s,
        Err(e) => return failed(e),
    };
    let home = net::home_addrs();
    let addrs = home
        .ipv4
        .iter()
        .map(|a| SocketAddr::from((*a, ctx.cfg.port)))
        .chain(
            home.ipv6
                .iter()
                .map(|a| SocketAddr::from((*a, ctx.cfg.port))),
        )
        .collect();
    let code = PairingCode {
        box_key: ctx.box_key.public().into(),
        secret,
        addrs,
    };
    Json(serde_json::json!({ "url": code.to_url(), "expires_in": PAIRING_LIFETIME }))
        .into_response()
}

async fn stop_pairing(State(ctx): State<Shared>) -> Response {
    ctx.devices.cancel_pairing();
    ctx.devices.cancel_laptop_pairing();
    StatusCode::NO_CONTENT.into_response()
}

async fn remove(State(ctx): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    let removed = match ctx.devices.remove(&id) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "not_found",
                "That device isn't paired.",
            );
        }
        Err(e) => return failed(e),
    };
    // Refused on every path at once.
    let cut = ctx
        .live
        .lock()
        .ok()
        .and_then(|l| l.as_ref().map(|l| l.cut(&removed.link.0)))
        .unwrap_or(0);
    info!(device = %id, cut, "device removed");
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct Rename {
    name: String,
}

#[derive(Deserialize)]
struct SetAway {
    on: bool,
}

/// Turns remote access away from home on or off. Off closes, at once, every
/// connection that is not over the home network.
async fn set_away(State(ctx): State<Shared>, Json(r): Json<SetAway>) -> Response {
    if let Err(e) = ctx.away.set(r.on) {
        return failed(e);
    }
    if !r.on
        && let Ok(mut reach) = ctx.reach.lock()
    {
        reach.published.clear();
        reach.published_at = None;
    }
    let cut = if r.on {
        0
    } else {
        ctx.live
            .lock()
            .ok()
            .and_then(|l| l.as_ref().map(|l| l.cut_away(false)))
            .unwrap_or(0)
    };
    info!(on = r.on, cut, "remote access away from home");
    StatusCode::NO_CONTENT.into_response()
}

async fn rename(
    State(ctx): State<Shared>,
    UrlPath(id): UrlPath<String>,
    Json(r): Json<Rename>,
) -> Response {
    if r.name.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "empty", "Give the device a name.");
    }
    match ctx.devices.rename(&id, &r.name) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "not_found",
            "That device isn't paired.",
        ),
        Err(e) => failed(e),
    }
}
