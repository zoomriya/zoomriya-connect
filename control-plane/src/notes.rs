//! Admin notes: an admin's phone marks the box stolen, or moves its name to a
//! new box, showing a note the box signed for it. The control plane checks the
//! box's signature on the note and the admin key's signature on the request,
//! so it never keeps a list of anyone's devices
//! (docs/architecture.md, "If the box is lost or restored").

#![allow(clippy::result_large_err)]

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use tracing::info;
use zoomriya_keys::{PublicKey, Purpose, open};
use zoomriya_proto::{AdminNote, MoveRequest, StolenRequest};

use crate::api::{Api, error, json, refuse, signed, slow_down};

/// Longest sealed note we read.
const MAX_NOTE: usize = 1024;

/// Opens the note, checks the box signed it and the request's signer is the
/// admin it names. Returns the box's key.
fn check_note(sealed: &str, signer: &PublicKey) -> Result<PublicKey, Response> {
    let note: AdminNote = open(Purpose::AdminNote, sealed, MAX_NOTE, |n: &AdminNote| {
        n.box_key.0
    })
    .map_err(|_| error(403, "bad_note", "This phone's admin note isn't valid."))?;
    if &note.admin.0 != signer {
        return Err(error(
            403,
            "bad_note",
            "This admin note was made for another phone.",
        ));
    }
    Ok(note.box_key.0)
}

/// `POST /v1/admin/stolen`
pub async fn stolen(
    State(api): State<Arc<Api>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !api.per_addr.allow(&peer.ip()) {
        return slow_down();
    }
    let signer = match signed(&api, &method, &uri, &headers, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    let req: StolenRequest = match json(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let box_key = match check_note(&req.note, &signer) {
        Ok(k) => k,
        Err(r) => return r,
    };
    match api.store.stolen_by_key(&box_key) {
        Ok(name) => {
            info!(%name, "marked stolen by an admin");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => refuse(e),
    }
}

/// `POST /v1/admin/move`
pub async fn move_name(
    State(api): State<Arc<Api>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !api.per_addr.allow(&peer.ip()) {
        return slow_down();
    }
    let signer = match signed(&api, &method, &uri, &headers, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    let req: MoveRequest = match json(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let old = match check_note(&req.note, &signer) {
        Ok(k) => k,
        Err(r) => return r,
    };
    if old == req.new_box_key.0 {
        return error(400, "same_box", "That's the same box.");
    }
    match api
        .store
        .move_name(&old, &req.new_box_key.0, &req.new_dns_update_key.0)
    {
        Ok(name) => {
            info!(%name, "name moved to a new box by an admin");
            (StatusCode::OK, Json(serde_json::json!({ "name": name }))).into_response()
        }
        Err(e) => refuse(e),
    }
}
