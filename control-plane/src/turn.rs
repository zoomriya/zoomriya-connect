//! One-hour credentials for the guests' relay (TURN), asked for by the box
//! after it has checked a guest's token. The control plane never sees the
//! token: only the box can check it (docs/architecture.md, "Guests' relay").
//!
//! The credentials are TURN's usual shared-secret kind: the username is when
//! they run out and a random part; the password is
//! `base64(HMAC-SHA1(secret, username))`. The TURN server checks them with
//! the same secret (`deploy/turnserver.conf`).

#![allow(clippy::result_large_err)]

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, Method, Uri},
    response::{IntoResponse, Response},
};
use data_encoding::{BASE64, HEXLOWER};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use zoomriya_proto::{TurnCredentials, names};

use crate::api::{Api, error, signed, slow_down};

/// How long credentials work.
pub const TTL: u64 = 3600;

/// The guests' relay.
#[derive(Debug, Clone)]
pub struct Turn {
    /// Its URLs.
    pub urls: Vec<String>,
    /// The shared secrets, newest first.
    pub secrets: Vec<String>,
}

impl Turn {
    /// Credentials good from `now` for an hour.
    pub fn credentials(&self, now: u64) -> Option<TurnCredentials> {
        let secret = self.secrets.first()?;
        let mut random = [0u8; 9];
        getrandom::fill(&mut random).ok()?;
        let username = format!("{}:{}", now + TTL, HEXLOWER.encode(&random));
        let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(username.as_bytes());
        Some(TurnCredentials {
            urls: self.urls.clone(),
            credential: BASE64.encode(&mac.finalize().into_bytes()),
            username,
            ttl: TTL,
        })
    }
}

/// `POST /v1/boxes/{name}/turn`, signed with the box key.
pub async fn credentials(
    State(api): State<Arc<Api>>,
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
    let Some(turn) = &api.turn else {
        return error(404, "no_guest_relay", "This server has no guests' relay.");
    };
    let key = match signed(&api, &method, &uri, &headers, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    match api.store.find(&names::normalise(&name)) {
        Ok(Some(row)) if row.box_key == key && !row.stolen && !row.disabled => {}
        _ => return error(403, "not_your_box", "This box can't ask for that name."),
    }
    if !api.per_box_turn.allow(&key) {
        return slow_down();
    }
    match turn.credentials((api.now)()) {
        Some(c) => Json(c).into_response(),
        None => error(500, "internal", "Something went wrong on our side."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_check_out() {
        let t = Turn {
            urls: vec!["turn:turn.example:3478".into()],
            secrets: vec!["s3cret".into(), "old".into()],
        };
        let c = t.credentials(1000).expect("credentials");
        let (expires, random) = c.username.split_once(':').expect("colon");
        assert_eq!(expires, "4600");
        assert_eq!(random.len(), 18);
        // What the TURN server computes.
        let mut mac = Hmac::<Sha1>::new_from_slice(b"s3cret").expect("mac");
        mac.update(c.username.as_bytes());
        assert_eq!(c.credential, BASE64.encode(&mac.finalize().into_bytes()));
        // Known answer for the scheme (RFC 2104 HMAC-SHA1).
        let mut mac = Hmac::<Sha1>::new_from_slice(b"key").expect("mac");
        mac.update(b"The quick brown fox jumps over the lazy dog");
        assert_eq!(
            HEXLOWER.encode(&mac.finalize().into_bytes()),
            "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9"
        );
        let none = Turn {
            urls: vec![],
            secrets: vec![],
        };
        assert!(none.credentials(0).is_none());
    }
}
