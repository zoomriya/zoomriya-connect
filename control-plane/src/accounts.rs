//! ZOOMRIYA Connect accounts: one per family, issued by an operator, separate
//! from ZOOMRIYA ID. A box links to one the first time it registers; the
//! family signs in to the account page to change its password and see its
//! boxes. An operator can reset the password or block the account.
//!
//! | Route | Who |
//! |---|---|
//! | `POST /v1/account/sign-in` `{username, password}` | The account page |
//! | `GET /v1/account` | The account page, signed in: the boxes and this month's use |
//! | `POST /v1/account/password` `{password, new_password}` | The account page, signed in |
//! | `POST /v1/account/sign-out` | The account page, signed in |
//! | `/account/` | The account page itself |
//!
//! Passwords are kept as Argon2id hashes. Five wrong passwords lock the
//! account for 15 minutes; sign-ins are limited per address too.

#![allow(clippy::result_large_err)]

use std::{net::SocketAddr, sync::Arc};

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use data_encoding::{BASE64URL_NOPAD, HEXLOWER};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::info;
use zoomriya_proto::{AccountLogin, month_of};

use crate::api::{Api, error, slow_down};

/// How long a sign-in on the account page lasts.
pub const SESSION: u64 = 12 * 3600;
/// Shortest password a family may pick.
pub const MIN_PASSWORD: usize = 10;
/// Boxes a new account may link, unless the operator says otherwise.
pub const DEFAULT_MAX_BOXES: u32 = 3;

/// Hashes a password with Argon2id and a random salt.
pub fn hash(password: &str) -> anyhow::Result<String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
    let h = Argon2::default()
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing: {e}"))?;
    Ok(h.to_string())
}

/// Whether `password` matches the stored hash.
pub fn verify(stored: &str, password: &str) -> bool {
    PasswordHash::new(stored).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

/// A password for an operator to hand over: four groups of four letters and
/// digits, without the ones that look alike (`0`/`o`, `1`/`l`/`i`).
pub fn new_password() -> anyhow::Result<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect();
    Ok(chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-"))
}

/// A username, as kept: lowercase, trimmed.
pub fn normalise(username: &str) -> String {
    username.trim().to_lowercase()
}

/// Whether a username is one we accept: 3 to 64 letters, digits and `.-_@+`.
pub fn valid_username(username: &str) -> bool {
    (3..=64).contains(&username.len())
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_@+".contains(c))
}

/// A hash compared against when the username doesn't exist, so the answer
/// takes as long as for a wrong password.
fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| hash("not a real password").unwrap_or_default())
}

/// Why a sign-in was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginRefused {
    /// Wrong username or password.
    Wrong,
    /// Too many wrong passwords lately.
    Locked,
    /// Blocked by an operator.
    Blocked,
}

impl LoginRefused {
    /// The answer, in plain words.
    pub fn response(self) -> Response {
        match self {
            Self::Wrong => error(401, "wrong_login", "That username or password isn't right."),
            Self::Locked => error(
                429,
                "account_locked",
                "Too many wrong passwords. Try again in 15 minutes.",
            ),
            Self::Blocked => error(
                403,
                "account_blocked",
                "This ZOOMRIYA Connect account is blocked. Ask whoever gave you the account.",
            ),
        }
    }
}

/// Checks a sign-in; returns the account's number and username.
pub async fn check(api: &Api, login: &AccountLogin) -> Result<(i64, String), LoginRefused> {
    let username = normalise(&login.username);
    let row = api.store.account(&username).ok().flatten();
    let stored = row
        .as_ref()
        .map_or_else(|| dummy_hash().to_string(), |r| r.password.clone());
    let password = login.password.clone();
    // Argon2 takes a moment on purpose: off the async threads.
    let ok = tokio::task::spawn_blocking(move || verify(&stored, &password))
        .await
        .unwrap_or(false);
    let Some(row) = row else {
        return Err(LoginRefused::Wrong);
    };
    let now = (api.now)();
    if row.locked_until > now {
        return Err(LoginRefused::Locked);
    }
    if !ok {
        let _ = api.store.wrong_password(row.id, now);
        return Err(LoginRefused::Wrong);
    }
    let _ = api.store.right_password(row.id);
    if row.blocked {
        return Err(LoginRefused::Blocked);
    }
    Ok((row.id, row.username))
}

fn token_hash(token: &str) -> String {
    HEXLOWER.encode(&Sha256::digest(token.as_bytes()))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// The signed-in account: number, username, most boxes.
fn signed_in(api: &Api, headers: &HeaderMap) -> Result<(i64, String, u32), Response> {
    bearer(headers)
        .and_then(|t| {
            api.store
                .session(&token_hash(t), (api.now)())
                .ok()
                .flatten()
        })
        .ok_or_else(|| error(401, "signed_out", "Sign in again."))
}

/// Signing in on the account page.
pub async fn sign_in(
    State(api): State<Arc<Api>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(login): Json<AccountLogin>,
) -> Response {
    if !api.per_sign_in.allow(&peer.ip()) {
        return slow_down();
    }
    if login.username.len() > 64 || login.password.len() > 200 {
        return LoginRefused::Wrong.response();
    }
    let (id, username) = match check(&api, &login).await {
        Ok(a) => a,
        Err(why) => return why.response(),
    };
    let mut bytes = [0u8; 32];
    if getrandom::fill(&mut bytes).is_err() {
        return error(500, "internal", "Something went wrong on our side.");
    }
    let token = format!("zcs_{}", BASE64URL_NOPAD.encode(&bytes));
    let now = (api.now)();
    if api
        .store
        .new_session(id, &token_hash(&token), now + SESSION, now)
        .is_err()
    {
        return error(500, "internal", "Something went wrong on our side.");
    }
    Json(serde_json::json!({ "token": token, "username": username })).into_response()
}

/// The account page: the account, its boxes, and this month's use.
pub async fn me(State(api): State<Arc<Api>>, headers: HeaderMap) -> Response {
    let (id, username, max_boxes) = match signed_in(&api, &headers) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let now = (api.now)();
    let month = month_of(now);
    let Ok(boxes) = api.store.account_boxes(id) else {
        return error(500, "internal", "Something went wrong on our side.");
    };
    let live: std::collections::HashMap<_, _> = api
        .seen
        .connected(now)
        .into_iter()
        .map(|(_, key, devices)| (key, devices))
        .collect();
    let boxes: Vec<serde_json::Value> = boxes
        .into_iter()
        .map(|(name, key, _)| {
            let (used, allowance) = api.store.relay_use(&key, &month).unwrap_or((0, 0));
            serde_json::json!({
                "name": name,
                "fqdn": format!("{name}.{}", api.zone),
                "connected": live.contains_key(&key),
                "devices": live.get(&key).copied().unwrap_or(0),
                "used": used,
                "allowance": allowance,
                "slowed": allowance > 0 && used > allowance,
            })
        })
        .collect();
    Json(serde_json::json!({
        "username": username,
        "max_boxes": max_boxes,
        "month": month,
        "boxes": boxes,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct NewPassword {
    password: String,
    new_password: String,
}

/// Changing the password: the old one again, then the new one. Every other
/// sign-in ends.
pub async fn change_password(
    State(api): State<Arc<Api>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(p): Json<NewPassword>,
) -> Response {
    let (_, username, _) = match signed_in(&api, &headers) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if !api.per_sign_in.allow(&peer.ip()) {
        return slow_down();
    }
    let login = AccountLogin {
        username: username.clone(),
        password: p.password,
    };
    if let Err(why) = check(&api, &login).await {
        return why.response();
    }
    if p.new_password.chars().count() < MIN_PASSWORD || p.new_password.len() > 200 {
        return error(
            400,
            "password_short",
            "Pick a password of at least 10 characters.",
        );
    }
    if p.new_password == login.password {
        return error(
            400,
            "password_same",
            "That's the password you have now. Pick a new one.",
        );
    }
    let new = p.new_password;
    let Ok(Ok(h)) = tokio::task::spawn_blocking(move || hash(&new)).await else {
        return error(500, "internal", "Something went wrong on our side.");
    };
    match api.store.set_password(&username, &h, (api.now)()) {
        Ok(true) => {
            info!(%username, "password changed by the family");
            StatusCode::NO_CONTENT.into_response()
        }
        _ => error(500, "internal", "Something went wrong on our side."),
    }
}

/// Signing out.
pub async fn sign_out(State(api): State<Arc<Api>>, headers: HeaderMap) -> Response {
    if let Some(t) = bearer(&headers) {
        let _ = api.store.end_session(&token_hash(t));
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords() {
        let h = hash("correct horse battery").expect("hash");
        assert!(h.starts_with("$argon2id$"));
        assert!(verify(&h, "correct horse battery"));
        assert!(!verify(&h, "correct horse battery!"));
        assert!(!verify("not a hash", "x"));
        let p = new_password().expect("password");
        assert_eq!(p.len(), 19);
        assert_eq!(p.matches('-').count(), 3);
        assert!(!p.contains(['0', 'o', '1', 'l', 'i']));
        assert_ne!(p, new_password().expect("another"));
    }

    #[test]
    fn usernames() {
        assert_eq!(
            normalise("  Anna.Smith@Example.ca "),
            "anna.smith@example.ca"
        );
        assert!(valid_username("anna.smith@example.ca"));
        assert!(valid_username("smiths-2"));
        assert!(!valid_username("ab"));
        assert!(!valid_username("anna smith"));
        assert!(!valid_username(&"a".repeat(65)));
    }
}
