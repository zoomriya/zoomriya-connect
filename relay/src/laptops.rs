//! Laptops away from home: the relay passes a laptop's WebRTC offer to its
//! box and the box's answer back, as they are (docs/laptops.md,
//! `zoomriya_proto::laptop`). It never reads them beyond the laptop's key:
//! the offer is signed by the laptop's device key and the answer sealed by
//! the box key, which each side checks itself.
//!
//! | Path | Who | Proof |
//! |---|---|---|
//! | `POST /zoomriya/v1/laptop/connect` | A laptop | Its pass, and the request signed with the link key the pass names |
//! | `POST /zoomriya/v1/laptop/wait` | A box, waiting for offers | Signed with the box key, which the relay knows |
//! | `POST /zoomriya/v1/laptop/answer` | A box, answering one | The same |

#![allow(clippy::result_large_err)]

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use tokio::sync::{Notify, oneshot};
use tracing::debug;
use zoomriya_keys::{PublicKey, REQUEST_SIGNATURE_HEADER, RequestSignature};
use zoomriya_proto::laptop::{
    LaptopOffer, RELAY_ANSWER, RELAY_CONNECT, RELAY_WAIT, RELAY_WAIT_SECONDS, RelayedAnswer,
    RelayedOffer, RelayedOffers,
};

use crate::front::{Api, error};

/// Biggest offer or answer.
pub const MAX_BODY: usize = 64 * 1024;
/// Offers waiting for one box at once.
const MAX_WAITING: usize = 16;
/// How long a laptop waits for its box's answer.
const ANSWER_WITHIN: Duration = Duration::from_secs(20);
/// A box counts as listening this long after its last wait.
const LISTENING: Duration = Duration::from_secs(60);

struct Listening {
    offers: VecDeque<RelayedOffer>,
    notify: Arc<Notify>,
    waits: u32,
    last: Instant,
}

impl Listening {
    fn new() -> Self {
        Self {
            offers: VecDeque::new(),
            notify: Arc::new(Notify::new()),
            waits: 0,
            last: Instant::now(),
        }
    }

    fn listening(&self) -> bool {
        self.waits > 0 || self.last.elapsed() < LISTENING
    }
}

#[derive(Default)]
struct Inner {
    boxes: HashMap<PublicKey, Listening>,
    answers: HashMap<u64, (PublicKey, oneshot::Sender<RelayedAnswer>)>,
}

/// The offers waiting for their boxes, and the laptops waiting for answers.
#[derive(Default)]
pub struct Hub {
    inner: Mutex<Inner>,
    next: AtomicU64,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("laptops::Hub")
    }
}

impl Hub {
    /// Queues an offer for a listening box. `None` if it isn't listening,
    /// or has too many waiting.
    fn offer(
        &self,
        box_key: PublicKey,
        offer: LaptopOffer,
    ) -> Result<(u64, oneshot::Receiver<RelayedAnswer>), Refused> {
        let mut inner = self.inner.lock().map_err(|_| Refused::Offline)?;
        let b = inner
            .boxes
            .get_mut(&box_key)
            .filter(|b| b.listening())
            .ok_or(Refused::Offline)?;
        if b.offers.len() >= MAX_WAITING {
            return Err(Refused::Busy);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        b.offers.push_back(RelayedOffer { id, offer });
        b.notify.notify_one();
        let (tx, rx) = oneshot::channel();
        inner.answers.insert(id, (box_key, tx));
        Ok((id, rx))
    }

    /// Forgets an offer nobody answered.
    fn give_up(&self, box_key: &PublicKey, id: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.answers.remove(&id);
            if let Some(b) = inner.boxes.get_mut(box_key) {
                b.offers.retain(|o| o.id != id);
            }
        }
    }

    /// The box's wait: the offers now, or the first to come within `within`.
    async fn wait(&self, box_key: PublicKey, within: Duration) -> RelayedOffers {
        let notify = {
            let Ok(mut inner) = self.inner.lock() else {
                return RelayedOffers::default();
            };
            let b = inner.boxes.entry(box_key).or_insert_with(Listening::new);
            b.waits += 1;
            b.notify.clone()
        };
        let deadline = tokio::time::Instant::now() + within;
        let offers = loop {
            let taken = self.take(&box_key);
            if !taken.is_empty() {
                break taken;
            }
            if tokio::time::timeout_at(deadline, notify.notified())
                .await
                .is_err()
            {
                break self.take(&box_key);
            }
        };
        if let Ok(mut inner) = self.inner.lock()
            && let Some(b) = inner.boxes.get_mut(&box_key)
        {
            b.waits = b.waits.saturating_sub(1);
            b.last = Instant::now();
        }
        RelayedOffers { offers }
    }

    fn take(&self, box_key: &PublicKey) -> Vec<RelayedOffer> {
        self.inner
            .lock()
            .ok()
            .and_then(|mut i| {
                i.boxes
                    .get_mut(box_key)
                    .map(|b| b.offers.drain(..).collect())
            })
            .unwrap_or_default()
    }

    /// The box's answer, to the laptop waiting for it. Only the box the
    /// offer was for may answer it.
    fn answer(&self, box_key: &PublicKey, answer: RelayedAnswer) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        match inner.answers.get(&answer.id) {
            Some((b, _)) if b == box_key => {}
            _ => return false,
        }
        let Some((_, tx)) = inner.answers.remove(&answer.id) else {
            return false;
        };
        tx.send(answer).is_ok()
    }

    /// Forgets boxes that stopped listening long ago.
    pub fn tidy(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner
                .boxes
                .retain(|_, b| b.waits > 0 || b.last.elapsed() < LISTENING * 10);
        }
    }
}

enum Refused {
    Offline,
    Busy,
}

/// Lets the extension's pages call us: they send no cookies, and every call
/// carries its own proof.
fn cors(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type, zoomriya-signature"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST"),
    );
    h.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    res
}

/// `OPTIONS /zoomriya/v1/laptop/connect`: the browser asking first.
pub async fn preflight() -> Response {
    cors(StatusCode::NO_CONTENT.into_response())
}

/// Reads and checks the request's signature; returns the key that signed it.
fn signed(api: &Api, headers: &HeaderMap, path: &str, body: &[u8]) -> Result<PublicKey, Response> {
    let sig = headers
        .get(REQUEST_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| RequestSignature::parse(v).ok())
        .ok_or_else(|| {
            error(
                StatusCode::UNAUTHORIZED,
                "unsigned",
                "The request isn't signed.",
            )
        })?;
    sig.verify(
        "POST",
        path,
        body,
        zoomriya_serve::now(),
        api.clock_allowance,
    )
    .map_err(|_| {
        error(
            StatusCode::UNAUTHORIZED,
            "unsigned",
            "The request's signature doesn't match. Check this computer's clock.",
        )
    })?;
    Ok(sig.key)
}

/// `POST /zoomriya/v1/laptop/connect`
pub async fn connect(State(api): State<Api>, headers: HeaderMap, body: Bytes) -> Response {
    cors(connect_inner(&api, &headers, &body).await)
}

async fn connect_inner(api: &Api, headers: &HeaderMap, body: &[u8]) -> Response {
    if body.len() > MAX_BODY {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_big",
            "The offer is too big.",
        );
    }
    let link = match signed(api, headers, RELAY_CONNECT, body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    let pass = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let box_key = match api.gate.decide(&link, pass) {
        Ok(b) if b != link => b,
        Ok(_) => {
            return error(
                StatusCode::FORBIDDEN,
                "not_a_laptop",
                "Boxes don't connect here.",
            );
        }
        Err(why) => {
            debug!("laptop refused: {why}");
            return error(
                StatusCode::FORBIDDEN,
                "pass",
                "This laptop's pass doesn't work. Connect at home once to get a new one.",
            );
        }
    };
    let Ok(offer) = serde_json::from_slice::<LaptopOffer>(body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "The offer can't be read.",
        );
    };
    if offer.link.0 != link {
        return error(
            StatusCode::FORBIDDEN,
            "pass",
            "The offer is for another laptop.",
        );
    }
    let (id, answer) = match api.laptops.offer(box_key, offer) {
        Ok(x) => x,
        Err(Refused::Offline) => {
            return error(
                StatusCode::NOT_FOUND,
                "box_offline",
                "Your box can't be reached right now. Check it's on and online.",
            );
        }
        Err(Refused::Busy) => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                "Your box is busy. Try again in a minute.",
            );
        }
    };
    match tokio::time::timeout(ANSWER_WITHIN, answer).await {
        Ok(Ok(RelayedAnswer {
            answer: Some(sealed),
            ..
        })) => Json(serde_json::json!({ "answer": sealed })).into_response(),
        Ok(Ok(RelayedAnswer { error: Some(e), .. })) => {
            (StatusCode::FORBIDDEN, Json(e)).into_response()
        }
        _ => {
            api.laptops.give_up(&box_key, id);
            error(
                StatusCode::GATEWAY_TIMEOUT,
                "no_answer",
                "Your box didn't answer. Try again.",
            )
        }
    }
}

/// Only registered boxes in good standing.
fn a_box(api: &Api, key: &PublicKey) -> Result<(), Response> {
    match api.gate.decide(key, None) {
        Ok(k) if k == *key => Ok(()),
        _ => Err(error(
            StatusCode::FORBIDDEN,
            "unknown_box",
            "This relay doesn't know your box.",
        )),
    }
}

/// `POST /zoomriya/v1/laptop/wait`
pub async fn wait(State(api): State<Api>, headers: HeaderMap, body: Bytes) -> Response {
    let key = match signed(&api, &headers, RELAY_WAIT, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    if let Err(r) = a_box(&api, &key) {
        return r;
    }
    Json(
        api.laptops
            .wait(key, Duration::from_secs(RELAY_WAIT_SECONDS))
            .await,
    )
    .into_response()
}

/// `POST /zoomriya/v1/laptop/answer`
pub async fn answer(State(api): State<Api>, headers: HeaderMap, body: Bytes) -> Response {
    let key = match signed(&api, &headers, RELAY_ANSWER, &body) {
        Ok(k) => k,
        Err(r) => return r,
    };
    if let Err(r) = a_box(&api, &key) {
        return r;
    }
    let Ok(a) = serde_json::from_slice::<RelayedAnswer>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "The answer can't be read.",
        );
    };
    if api.laptops.answer(&key, a) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        error(StatusCode::GONE, "gone", "The laptop stopped waiting.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoomriya_keys::SecretKey;

    fn offer(link: &SecretKey) -> LaptopOffer {
        LaptopOffer {
            link: link.public().into(),
            time: 1,
            sdp: "v=0".into(),
            sig: String::new(),
        }
    }

    #[tokio::test]
    async fn offers_reach_their_box_and_answers_come_back() {
        let hub = Arc::new(Hub::default());
        let b = SecretKey::generate().public();
        let other = SecretKey::generate().public();
        let laptop = SecretKey::generate();

        // Nobody listening yet.
        assert!(matches!(
            hub.offer(b, offer(&laptop)),
            Err(Refused::Offline)
        ));

        // The box waits; an offer comes.
        let h = hub.clone();
        let waiting = tokio::spawn(async move { h.wait(b, Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (id, rx) = hub.offer(b, offer(&laptop)).ok().expect("queued");
        let got = waiting.await.expect("wait");
        assert_eq!(got.offers.len(), 1);
        assert_eq!(got.offers[0].id, id);

        // Another box can't answer it; its own box can.
        let a = RelayedAnswer {
            id,
            answer: Some("sealed".into()),
            error: None,
        };
        assert!(!hub.answer(&other, a.clone()));
        assert!(hub.answer(&b, a));
        assert_eq!(rx.await.expect("answer").answer.as_deref(), Some("sealed"));

        // Still counted as listening right after its wait.
        let (id, _rx) = hub.offer(b, offer(&laptop)).ok().expect("queued");
        hub.give_up(&b, id);
        assert!(hub.take(&b).is_empty());
    }

    #[tokio::test]
    async fn an_empty_wait_ends_in_time() {
        let hub = Hub::default();
        let b = SecretKey::generate().public();
        let got = hub.wait(b, Duration::from_millis(50)).await;
        assert!(got.offers.is_empty());
    }
}
