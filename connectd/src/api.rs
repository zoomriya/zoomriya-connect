//! Box Desktop's way into connectd: a Unix socket that only box-desktopd's
//! group can open. Box Desktop's own sign-in decides who may use it (admins).
//!
//! | Route | What |
//! |---|---|
//! | `GET /v1/status` | On or off, the name, the relay, the certificate, problems |
//! | `POST /v1/setup` `{name, username, password}` | Picks the box's name and registers, linking the box to its ZOOMRIYA Connect account |
//! | `POST /v1/on`, `POST /v1/off` | Switches ZOOMRIYA Connect on or off |
//! | `POST /v1/release` | Gives the name up |
//! | `GET /v1/devices` | Paired devices, with how each connects |
//! | `POST /v1/pairing` `{name, admin}` | A pairing code for the QR code |
//! | `DELETE /v1/pairing` | Stops pairing |
//! | `DELETE /v1/devices/{id}` | Removes a device, at once, everywhere |
//! | `POST /v1/devices/{id}/name` `{name}` | Renames a device |
//! | `POST /v1/server` `{url}` | Uses another ZOOMRIYA Connect server (or `null` for ours) |
//! | `POST /v1/relay` `{off}` | Switches the relay off or on |

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tracing::info;
use zoomriya_proto::{
    ErrorBody,
    link::PairingCode,
    names::{self, NameRules},
};

use crate::{
    daemon::{Ctx, Health},
    devices::{Device, PAIRING_LIFETIME},
    net,
    server::{self, Server},
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

fn from_server(e: anyhow::Error) -> Response {
    match server::said(&e) {
        Some(s) => error(
            StatusCode::from_u16(s.status).unwrap_or(StatusCode::BAD_GATEWAY),
            &s.code,
            &s.message,
        ),
        None => error(
            StatusCode::BAD_GATEWAY,
            "unreachable",
            "The box can't reach the ZOOMRIYA Connect server. Check the internet connection, then try again.",
        ),
    }
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
        .route("/v1/setup", post(setup))
        .route("/v1/on", post(on))
        .route("/v1/off", post(off))
        .route("/v1/release", post(release))
        .route("/v1/devices", get(devices))
        .route("/v1/devices/{id}", delete(remove))
        .route("/v1/devices/{id}/name", post(rename))
        .route("/v1/pairing", post(start_pairing).delete(stop_pairing))
        .route("/v1/server", post(set_server))
        .route("/v1/relay", post(set_relay))
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
    on: bool,
    name: Option<String>,
    fqdn: Option<String>,
    account: Option<String>,
    server: String,
    our_server: bool,
    relay_off: bool,
    box_key: String,
    devices: usize,
    connected: usize,
    pairing: bool,
    clock_synced: bool,
    #[serde(flatten)]
    health: Health,
}

async fn status(State(ctx): State<Shared>) -> Response {
    let st = ctx.state.get();
    let devices = ctx
        .devices
        .all()
        .iter()
        .filter(|d| d.removed_at.is_none())
        .count();
    let connected = ctx
        .live
        .lock()
        .ok()
        .and_then(|l| l.as_ref().map(|l| l.count()))
        .unwrap_or(0);
    Json(Status {
        on: !st.off && st.name.is_some(),
        name: st.name.clone(),
        fqdn: st.fqdn.clone(),
        account: st.account.clone(),
        our_server: st.server.is_none(),
        server: ctx.server_url(),
        relay_off: st.relay_off,
        box_key: ctx.box_key.public().to_z32(),
        devices,
        connected,
        pairing: ctx.devices.pairing_open(crate::now()),
        clock_synced: crate::clock::synced(),
        health: ctx.health(|h| h.clone()).unwrap_or_default(),
    })
    .into_response()
}

#[derive(Deserialize)]
struct Setup {
    name: String,
    /// The ZOOMRIYA Connect account's username and password, sent to the
    /// server once to link the box, and never kept.
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

async fn setup(State(ctx): State<Shared>, Json(req): Json<Setup>) -> Response {
    let name = names::normalise(&req.name);
    if let Err(why) = NameRules::new().check(&name) {
        return error(StatusCode::BAD_REQUEST, why.code(), why.message());
    }
    let server = match Server::new(&ctx.server_url()) {
        Ok(s) => s,
        Err(e) => return failed(e),
    };
    let mut request = ctx.register_request(&name);
    if let (Some(username), Some(password)) = (req.username, req.password)
        && !username.trim().is_empty()
    {
        request.account = Some(zoomriya_proto::AccountLogin {
            username: username.trim().to_string(),
            password,
        });
    }
    match server.register(&ctx.box_key, &request).await {
        Ok(r) => {
            let saved = ctx.state.update(|s| {
                s.name = Some(r.name.clone());
                s.fqdn = Some(r.fqdn.clone());
                s.share_fqdn = Some(r.share_fqdn.clone());
                s.account = r.account.clone();
                s.off = false;
            });
            if let Err(e) = saved {
                return failed(e);
            }
            ctx.health(|h| {
                h.problems.remove("account");
                h.problems.remove("server");
            });
            info!(fqdn = %r.fqdn, "box named");
            ctx.changed.notify_one();
            Json(r).into_response()
        }
        Err(e) => from_server(e),
    }
}

async fn on(State(ctx): State<Shared>) -> Response {
    switch(&ctx, false)
}

async fn off(State(ctx): State<Shared>) -> Response {
    switch(&ctx, true)
}

fn switch(ctx: &Ctx, off: bool) -> Response {
    match ctx.state.update(|s| s.off = off) {
        Ok(()) => {
            ctx.changed.notify_one();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => failed(e),
    }
}

async fn release(State(ctx): State<Shared>) -> Response {
    let Some(name) = ctx.state.get().name else {
        return error(
            StatusCode::CONFLICT,
            "no_name",
            "The box has no name to give up.",
        );
    };
    let server = match Server::new(&ctx.server_url()) {
        Ok(s) => s,
        Err(e) => return failed(e),
    };
    if let Err(e) = server.release(&ctx.box_key, &name).await {
        return from_server(e);
    }
    let _ = std::fs::remove_file(ctx.cfg.certs_dir.join(crate::acme::BUNDLE));
    match ctx.state.update(|s| {
        s.name = None;
        s.fqdn = None;
        s.account = None;
        s.share_fqdn = None;
    }) {
        Ok(()) => {
            ctx.changed.notify_one();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => failed(e),
    }
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
        .filter(|d| d.removed_at.is_none())
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
}

async fn start_pairing(State(ctx): State<Shared>, Json(req): Json<NewPairing>) -> Response {
    let st = ctx.state.get();
    let (Some(fqdn), false) = (st.fqdn.clone(), st.off) else {
        return error(
            StatusCode::CONFLICT,
            "not_on",
            "Switch ZOOMRIYA Connect on before pairing a phone.",
        );
    };
    if !crate::clock::synced() {
        return error(
            StatusCode::CONFLICT,
            "clock",
            "Your box can't get the time from the internet yet. Check its internet connection, then try again.",
        );
    }
    let now = crate::now();
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
        fqdn,
        addrs,
        server: ctx.server_url(),
    };
    Json(serde_json::json!({ "url": code.to_url(), "expires_in": PAIRING_LIFETIME }))
        .into_response()
}

async fn stop_pairing(State(ctx): State<Shared>) -> Response {
    ctx.devices.cancel_pairing();
    StatusCode::NO_CONTENT.into_response()
}

async fn remove(State(ctx): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    let removed = match ctx.devices.remove(&id, crate::now()) {
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
    // Refused on every path at once, and on the relay as soon as it hears.
    let cut = ctx
        .live
        .lock()
        .ok()
        .and_then(|l| l.as_ref().map(|l| l.cut(&removed.link.0)))
        .unwrap_or(0);
    info!(device = %id, cut, "device removed");
    let ctx2 = ctx.clone();
    tokio::spawn(async move { ctx2.send_removed(false, Some(removed)).await });
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct Rename {
    name: String,
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

#[derive(Deserialize)]
struct SetServer {
    url: Option<String>,
}

async fn set_server(State(ctx): State<Shared>, Json(req): Json<SetServer>) -> Response {
    let url = req.url.map(|u| u.trim().trim_end_matches('/').to_string());
    if let Some(u) = &url {
        if !u.starts_with("https://") {
            return error(
                StatusCode::BAD_REQUEST,
                "not_https",
                "Type your server's address starting with https://",
            );
        }
        // Check it answers like a ZOOMRIYA Connect server before switching.
        let ok = match Server::new(u) {
            Ok(s) => s.well_known().await.is_ok(),
            Err(_) => false,
        };
        if !ok {
            return error(
                StatusCode::BAD_REQUEST,
                "not_a_server",
                "That address doesn't answer like a ZOOMRIYA Connect server.",
            );
        }
    }
    // A new server means a new registration, and phones pair again.
    match ctx.state.update(|s| {
        s.server = url;
        s.name = None;
        s.fqdn = None;
        s.account = None;
        s.share_fqdn = None;
    }) {
        Ok(()) => {
            ctx.changed.notify_one();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => failed(e),
    }
}

#[derive(Deserialize)]
struct SetRelay {
    off: bool,
}

async fn set_relay(State(ctx): State<Shared>, Json(req): Json<SetRelay>) -> Response {
    match ctx.state.update(|s| s.relay_off = req.off) {
        Ok(()) => {
            ctx.changed.notify_one();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => failed(e),
    }
}
