//! Our relays' monthly allowance: each relay reports what each box used and
//! who is connected; we add up the month and say which boxes to slow. A box
//! asks for its own use, for Box Desktop.
//!
//! | Route | Who | Signed with |
//! |---|---|---|
//! | `POST /v1/relay/report` | Relays, every poll: each box's relay and TURN bytes, who's connected, how the server is doing | Control plane to relay key |
//! | `POST /v1/boxes/{name}/relay-use` | The box, for Box Desktop | Box key |

#![allow(clippy::result_large_err)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, Method, Uri},
    response::{IntoResponse, Response},
};
use zoomriya_keys::PublicKey;
use zoomriya_proto::{BoxConnected, RelayReport, RelayReportAnswer, RelayUse, month_of, names};

use crate::api::{Api, error, is_relay, json, refuse, signed, slow_down};

/// A relay's report counts as current for this long.
pub const SEEN_FOR: u64 = 120;
/// Most boxes in one report.
pub const MAX_REPORT: usize = 200_000;

/// What each relay last said is connected.
#[derive(Debug, Default)]
pub struct Seen(Mutex<HashMap<String, (u64, Vec<BoxConnected>)>>);

impl Seen {
    /// Keeps a relay's list.
    pub fn put(&self, relay: &str, now: u64, connected: Vec<BoxConnected>) {
        if let Ok(mut s) = self.0.lock() {
            s.insert(relay.to_string(), (now, connected));
        }
    }

    /// Every box connected now: relay, box key, devices. Relays that haven't
    /// reported for two minutes are left out.
    pub fn connected(&self, now: u64) -> Vec<(String, PublicKey, u32)> {
        let Ok(s) = self.0.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (relay, (at, list)) in s.iter() {
            if now.saturating_sub(*at) > SEEN_FOR {
                continue;
            }
            for c in list {
                out.push((relay.clone(), c.key.0, c.devices));
            }
        }
        out
    }

    /// Relays that reported lately: name, seconds since 1970.
    pub fn relays(&self) -> Vec<(String, u64)> {
        self.0
            .lock()
            .map(|s| s.iter().map(|(r, (at, _))| (r.clone(), *at)).collect())
            .unwrap_or_default()
    }
}

/// A relay's report.
pub async fn report(State(api): State<Arc<Api>>, headers: HeaderMap, body: Bytes) -> Response {
    if !is_relay(&api, &headers) {
        return error(401, "not_a_relay", "Only our relays can send this.");
    }
    let r: RelayReport = match json(&body) {
        Ok(r) => r,
        Err(res) => return res,
    };
    if r.relay.is_empty()
        || r.relay.len() > 253
        || r.used.len() > MAX_REPORT
        || r.connected.len() > MAX_REPORT
    {
        return error(400, "bad_request", "The report can't be read.");
    }
    let now = (api.now)();
    let month = month_of(now);
    let used: Vec<(PublicKey, u64)> = r
        .used
        .iter()
        .filter(|u| u.bytes > 0)
        .map(|u| (u.key.0, u.bytes))
        .collect();
    let turn: Vec<(PublicKey, u64)> = r
        .turn
        .iter()
        .filter(|u| u.bytes > 0)
        .map(|u| (u.key.0, u.bytes))
        .collect();
    let s = &api.store;
    if let Err(e) = s
        .add_relay_use(&month, &used)
        .and_then(|()| s.add_turn_use(&month, &turn))
    {
        tracing::error!("relay use: {e:#}");
        return error(500, "internal", "Something went wrong on our side.");
    }
    if let Some(mut stats) = r.server {
        // The name and role are the report's, whatever the stats say.
        stats.name = r.relay.clone();
        stats.role = "relay".into();
        let sum = |v: &[(PublicKey, u64)]| v.iter().fold(0u64, |t, (_, b)| t.saturating_add(*b));
        if let Err(e) = s.server_report(&stats, now, sum(&used), sum(&turn)) {
            tracing::warn!("server report: {e:#}");
        }
    }
    api.seen.put(&r.relay, now, r.connected);
    match (s.over_allowance(&month), s.allowances()) {
        (Ok(slow), Ok(a)) => Json(RelayReportAnswer {
            slow: slow.into_iter().map(zoomriya_keys::Key).collect(),
            slow_bytes_per_second: a.slow_kbps.saturating_mul(1000),
        })
        .into_response(),
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("allowances: {e:#}");
            error(500, "internal", "Something went wrong on our side.")
        }
    }
}

/// A box asks for its use this month.
pub async fn box_use(
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
    let key = match signed(&api, &method, &uri, &headers, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    let name = names::normalise(&name);
    match api.store.find(&name) {
        Ok(Some(row)) if row.box_key == key && !row.stolen => {}
        Ok(_) => return error(403, "not_your_box", "That isn't this box's name."),
        Err(e) => return refuse(e),
    }
    let month = month_of((api.now)());
    match api.store.relay_use(&key, &month) {
        Ok((used, allowance)) => Json(RelayUse {
            slowed: allowance > 0 && used > allowance,
            month,
            used,
            allowance,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("relay use: {e:#}");
            error(500, "internal", "Something went wrong on our side.")
        }
    }
}
