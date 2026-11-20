//! The second DNS server (`ns2.zoomriya.net`), at another hosting company:
//! it copies what the first answers over HTTPS and answers DNS from the copy.
//! It keeps nothing on disk and has no API, so there's nothing on it to break
//! into but DNS itself.
//!
//! The first server serves `GET /v1/zone` to holders of a zone-copy key, with
//! the zone's serial as its `ETag`: an unchanged zone is a `304`. The second
//! server reports how it's doing with the same key (`POST /v1/server/report`),
//! for the GUI's Servers page.

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;
use tracing::{info, warn};
use zoomriya_proto::{ServerStats, ZoneCopy};

use crate::{
    api::{Api, error},
    store::Store,
};

/// Whether the request carries one of our name servers' zone-copy keys.
fn from_name_server(api: &Api, headers: &HeaderMap) -> bool {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    !token.is_empty()
        && api.zone_keys.iter().fold(false, |hit, k| {
            hit | bool::from(k.as_bytes().ct_eq(token.as_bytes()))
        })
}

/// `POST /v1/server/report`: the second DNS server says how it's doing, for
/// the GUI's Servers page.
pub async fn server_report(
    State(api): State<Arc<Api>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !from_name_server(&api, &headers) {
        return error(
            401,
            "not_a_name_server",
            "Only our name servers can send this.",
        );
    }
    let Ok(mut stats) = serde_json::from_slice::<ServerStats>(&body) else {
        return error(400, "bad_request", "The report can't be read.");
    };
    if stats.name.is_empty() || stats.name.len() > 253 {
        return error(400, "bad_request", "The report can't be read.");
    }
    stats.role = "second DNS".into();
    stats.turn = None;
    match api.store.server_report(&stats, (api.now)(), 0, 0) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            warn!("server report: {e:#}");
            error(500, "internal", "Something went wrong on our side.")
        }
    }
}

/// `GET /v1/zone`
pub async fn copy(State(api): State<Arc<Api>>, headers: HeaderMap) -> Response {
    if !from_name_server(&api, &headers) {
        return error(
            401,
            "not_a_name_server",
            "Only our name servers can ask for this.",
        );
    }
    let etag = format!("\"{}\"", api.store.serial());
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let copy = api.store.zone_copy();
    ([(header::ETAG, etag)], Json(copy)).into_response()
}

/// Keeps `store` a copy of the first server's zone, forever, and every half
/// minute tells the first server how this one is doing: `me` is its name
/// and when it started.
pub async fn follow(
    store: Arc<Store>,
    primary: String,
    key: String,
    every: Duration,
    me: (String, u64),
) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("zoomriya-ns2/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            warn!("can't make an HTTP client: {err}");
            return;
        }
    };
    let url = format!("{}/v1/zone", primary.trim_end_matches('/'));
    let report_url = format!("{}/v1/server/report", primary.trim_end_matches('/'));
    let mut etag: Option<String> = None;
    let mut failures = 0u32;
    let mut reported: Option<std::time::Instant> = None;
    loop {
        if reported.is_none_or(|t| t.elapsed() >= Duration::from_secs(30)) {
            reported = Some(std::time::Instant::now());
            let stats = zoomriya_serve::host::stats(
                &me.0,
                "second DNS",
                env!("CARGO_PKG_VERSION"),
                me.1,
                std::path::Path::new("/"),
            );
            if let Err(err) = client
                .post(&report_url)
                .bearer_auth(&key)
                .json(&stats)
                .send()
                .await
            {
                tracing::debug!("server report: {err}");
            }
        }
        let mut req = client.get(&url).bearer_auth(&key);
        if let Some(e) = &etag {
            req = req.header(reqwest::header::IF_NONE_MATCH, e);
        }
        match req.send().await {
            Ok(res) if res.status() == reqwest::StatusCode::NOT_MODIFIED => failures = 0,
            Ok(res) if res.status().is_success() => {
                let tag = res
                    .headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);
                match res.json::<ZoneCopy>().await {
                    Ok(copy) => {
                        store.take_zone_copy(&copy);
                        if etag.is_none() {
                            info!(names = copy.names.len(), "zone copied");
                        }
                        etag = tag;
                        failures = 0;
                    }
                    Err(err) => warn!("zone copy: {err}"),
                }
            }
            Ok(res) => {
                failures += 1;
                if failures == 1 || failures.is_multiple_of(12) {
                    warn!(
                        "zone copy: {} (answering from the copy we have)",
                        res.status()
                    );
                }
            }
            Err(err) => {
                failures += 1;
                if failures == 1 || failures.is_multiple_of(12) {
                    warn!("zone copy: {err} (answering from the copy we have)");
                }
            }
        }
        tokio::time::sleep(every).await;
    }
}
