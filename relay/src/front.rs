//! The relay's front door on port 443. `/relay` goes to iroh's relay service
//! untouched; everything else is ours: the box's removed-device list, a
//! family relay's enrolment, laptops' offers from away (`laptops`), health
//! and the probe iroh clients use.

use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use http_body_util::Either;
use hyper::{body::Incoming, service::Service};
use hyper_util::rt::TokioIo;
use iroh_relay::{
    http::{RELAY_PATH, RELAY_PROBE_PATH},
    server::{
        http_server::{BytesBody, HyperError, RelayService, RelayServiceWithNotify},
        streams::MaybeTlsStream,
    },
};
use tokio::{
    net::{TcpListener, TcpSocket},
    sync::{Mutex, Notify},
};
use tower::ServiceExt;
use tracing::{debug, info, warn};
use zoomriya_keys::{Purpose, REQUEST_SIGNATURE_HEADER, RequestSignature, open};
use zoomriya_proto::{BoxState, EnrolRequest, ErrorBody, RemovedDevices, laptop};
use zoomriya_serve::TlsListener;

use crate::{
    boxes, enrol,
    gate::{Gate, ListRefused},
    laptops,
    meter::{CONN_HEADER, Meter},
    sni::{self, Sni},
    turn,
};

/// Largest removed-device list we read.
const MAX_LIST: usize = 128 * 1024;
/// How long a connection has to become a relay connection, or finish its API call.
const ESTABLISH: Duration = Duration::from_secs(30);

/// What the API needs.
#[derive(Debug, Clone)]
pub struct Api {
    /// The gate.
    pub gate: Arc<Gate>,
    /// Set for a family's own relay: where codes and the list of boxes live.
    pub enrolled: Option<Enrolled>,
    /// Clock allowance, in seconds.
    pub clock_allowance: u64,
    /// Laptops' offers from away, waiting for their boxes.
    pub laptops: Arc<laptops::Hub>,
    /// The TURN server beside this relay, if there is one.
    pub turn: Option<Arc<turn::Turn>>,
}

/// A family relay's enrolment state.
#[derive(Debug, Clone)]
pub struct Enrolled {
    /// The state directory.
    pub state_dir: PathBuf,
    /// One enrolment at a time.
    pub lock: Arc<Mutex<()>>,
}

pub(crate) fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            code: code.into(),
            message: message.into(),
        }),
    )
        .into_response()
}

/// Our routes.
pub fn router(api: Api) -> Router {
    Router::new()
        .route("/", get(|| async { "ZOOMRIYA Connect relay\n" }))
        .route("/healthz", get(|| async { "ok\n" }))
        .route(
            "/robots.txt",
            get(|| async { "User-agent: *\nDisallow: /\n" }),
        )
        .route(
            RELAY_PROBE_PATH,
            get(|| async { ([(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")], "") }),
        )
        .route("/zoomriya/v1/removed", post(removed))
        .route("/zoomriya/v1/enrol", post(enrol_box))
        .route(
            laptop::RELAY_CONNECT,
            post(laptops::connect).options(laptops::preflight),
        )
        .route(laptop::RELAY_WAIT, post(laptops::wait))
        .route(laptop::RELAY_ANSWER, post(laptops::answer))
        .route(zoomriya_proto::RELAY_TURN, post(turn::credentials))
        .layer(DefaultBodyLimit::max(MAX_LIST))
        .with_state(api)
}

async fn removed(State(api): State<Api>, body: String) -> Response {
    let list: RemovedDevices = match open(
        Purpose::RemovedDevices,
        body.trim(),
        MAX_LIST,
        |l: &RemovedDevices| l.box_key.0,
    ) {
        Ok(l) => l,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "bad_list",
                "The list isn't signed by its box.",
            );
        }
    };
    match api.gate.take_removed(&list) {
        Ok(dropped) => {
            info!(box_key = %list.box_key, devices = list.devices.len(), dropped, "removed devices");
            Json(serde_json::json!({ "dropped": dropped })).into_response()
        }
        Err(ListRefused::UnknownBox) => error(
            StatusCode::FORBIDDEN,
            "unknown_box",
            "This relay doesn't know your box.",
        ),
        Err(ListRefused::Stale) => {
            error(StatusCode::CONFLICT, "stale", "A newer list arrived first.")
        }
        Err(ListRefused::Clock) => error(
            StatusCode::BAD_REQUEST,
            "clock",
            "Your box's clock is too far off. Check its internet connection.",
        ),
        Err(ListRefused::TooMany) => error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_many",
            "Too many removed devices in one list.",
        ),
    }
}

async fn enrol_box(State(api): State<Api>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(enrolled) = &api.enrolled else {
        return error(
            StatusCode::NOT_FOUND,
            "not_enrolling",
            "This relay gets its boxes from its control plane.",
        );
    };
    let Some(sig) = headers
        .get(REQUEST_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| RequestSignature::parse(v).ok())
    else {
        return error(
            StatusCode::UNAUTHORIZED,
            "unsigned",
            "The request isn't signed.",
        );
    };
    let Ok(req) = serde_json::from_slice::<EnrolRequest>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "The request can't be read.",
        );
    };
    if sig.key != req.box_key.0
        || sig
            .verify(
                "POST",
                "/zoomriya/v1/enrol",
                &body,
                zoomriya_serve::now(),
                api.clock_allowance,
            )
            .is_err()
    {
        return error(
            StatusCode::UNAUTHORIZED,
            "unsigned",
            "The request isn't signed by the box.",
        );
    }
    let _one_at_a_time = enrolled.lock.lock().await;
    if let Err(why) = enrol::redeem(&enrolled.state_dir, &req.code, zoomriya_serve::now()) {
        return error(StatusCode::FORBIDDEN, "bad_code", &why.to_string());
    }
    api.gate.apply(req.box_key.0, BoxState::Active);
    if let Err(err) = boxes::save(&boxes::path(&enrolled.state_dir), 0, &api.gate) {
        warn!("saving boxes: {err:#}");
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "save",
            "The relay couldn't save your box. Try again.",
        );
    }
    info!(box_key = %req.box_key, "box enrolled");
    StatusCode::NO_CONTENT.into_response()
}

/// Sends `/relay` to iroh, everything else to our router.
#[derive(Clone)]
struct Front {
    relay: RelayServiceWithNotify,
    api: Router,
    peer: SocketAddr,
    /// The loopback port this connection reaches us from: the meter's name for it.
    port: u16,
}

type FrontBody = Either<BytesBody, Body>;

impl Service<hyper::Request<Incoming>> for Front {
    type Response = hyper::Response<FrontBody>;
    type Error = HyperError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, mut req: hyper::Request<Incoming>) -> Self::Future {
        if req.uri().path() == RELAY_PATH {
            // Replaces anything the client sent under the same name.
            req.headers_mut()
                .insert(CONN_HEADER, header::HeaderValue::from(self.port));
            let fut = self.relay.call(req);
            Box::pin(async move { fut.await.map(|r| r.map(Either::Left)) })
        } else {
            req.headers_mut().remove(CONN_HEADER);
            req.extensions_mut()
                .insert(axum::extract::ConnectInfo(self.peer));
            let api = self.api.clone();
            Box::pin(async move {
                let res: Result<Response, Infallible> = api.oneshot(req.map(Body::new)).await;
                let Ok(res) = res;
                Ok(res.map(Either::Right))
            })
        }
    }
}

/// Where connections for other names on port 443 go.
#[derive(Debug, Clone, Default)]
pub struct Routes {
    /// Names passed on unopened: name, where.
    pub pass_through: Vec<(String, SocketAddr)>,
    /// TURN over TLS: its name, and coturn's TCP port on this machine.
    pub turn: Option<(String, SocketAddr)>,
}

impl Routes {
    fn pass_to(&self, name: &str) -> Option<SocketAddr> {
        self.pass_through
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, to)| *to)
    }
}

/// Reads the name a connection asks for, without taking anything from it.
async fn peek_name(tcp: &tokio::net::TcpStream) -> Option<String> {
    let mut buf = vec![0u8; sni::MAX_HELLO + 5];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let n = tokio::time::timeout_at(deadline, tcp.peek(&mut buf))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            return None;
        }
        match sni::read(&buf[..n]) {
            Sni::Name(name) => return Some(name),
            Sni::None => return None,
            Sni::More if n >= buf.len() => return None,
            Sni::More => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
}

/// Copies both ways until either side closes.
async fn splice<A, B>(mut a: A, mut b: B)
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
}

/// Serves HTTPS forever. A connection for a passed-through name goes on
/// unopened; one for the TURN name goes to coturn once its TLS is opened;
/// every other TLS connection is passed on, counted, over loopback to the
/// inner side, where iroh and our API answer it.
pub async fn serve(
    listener: TlsListener,
    relay: RelayService,
    api: Router,
    meter: Arc<Meter>,
    routes: Routes,
) {
    let inner = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await {
        Ok(l) => l,
        Err(err) => {
            warn!("can't listen on loopback: {err}");
            return;
        }
    };
    let Ok(inner_addr) = inner.local_addr() else {
        return;
    };
    tokio::spawn(serve_inner(inner, relay, api, meter.clone()));
    let listener = Arc::new(listener);
    loop {
        let (tcp, peer) = match listener.accept().await {
            Ok(c) => c,
            Err(err) => {
                warn!("accept: {err}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let _ = tcp.set_nodelay(true);
        let listener = listener.clone();
        let meter = meter.clone();
        let routes = routes.clone();
        tokio::spawn(async move {
            // Another program's name on this machine: passed on unopened.
            if !routes.pass_through.is_empty()
                && let Some(to) = peek_name(&tcp).await.and_then(|n| routes.pass_to(&n))
            {
                match tokio::net::TcpStream::connect(to).await {
                    Ok(up) => splice(tcp, up).await,
                    Err(err) => debug!("pass through to {to}: {err}"),
                }
                return;
            }
            let tls = match tokio::time::timeout(ESTABLISH, listener.handshake(tcp)).await {
                Ok(Ok(Some(tls))) => tls,
                Ok(Ok(None)) => return,
                Ok(Err(err)) => {
                    debug!("TLS from {peer}: {err}");
                    return;
                }
                Err(_) => return,
            };
            // TURN over TLS: the relay's certificate covers its name; what's
            // inside goes to coturn on this machine. WebRTC's own encryption
            // runs inside it, between the laptop and the box.
            if let Some((name, to)) = &routes.turn
                && tls
                    .get_ref()
                    .1
                    .server_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            {
                match tokio::net::TcpStream::connect(to).await {
                    Ok(up) => splice(tls, up).await,
                    Err(err) => debug!("TURN on {to}: {err}"),
                }
                return;
            }
            let Ok(sock) = TcpSocket::new_v4() else {
                return;
            };
            let port = match sock
                .bind((Ipv4Addr::LOCALHOST, 0).into())
                .and_then(|()| sock.local_addr())
            {
                Ok(a) => a.port(),
                Err(err) => {
                    warn!("loopback: {err}");
                    return;
                }
            };
            let conn = meter.open(port, peer);
            if let Ok(to_iroh) = sock.connect(inner_addr).await {
                let _ = to_iroh.set_nodelay(true);
                meter.pump(tls, to_iroh, &conn).await;
            }
            meter.close(port);
        });
    }
}

/// The inner side: loopback connections from [`serve`] only.
async fn serve_inner(inner: TcpListener, relay: RelayService, api: Router, meter: Arc<Meter>) {
    loop {
        let (tcp, from) = match inner.accept().await {
            Ok(c) => c,
            Err(err) => {
                warn!("loopback accept: {err}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        // Only our own outer side, which noted the port first.
        let Some(conn) = from
            .ip()
            .is_loopback()
            .then(|| meter.get(from.port()))
            .flatten()
        else {
            continue;
        };
        let _ = tcp.set_nodelay(true);
        let relay = relay.clone();
        let api = api.clone();
        tokio::spawn(async move {
            let established = Arc::new(Notify::new());
            let front = Front {
                relay: RelayServiceWithNotify::new(relay, established.clone()),
                api,
                peer: conn.peer,
                port: from.port(),
            };
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(MaybeTlsStream::Plain(tcp)), front)
                .with_upgrades();
            // A connection that hasn't become a relay connection within the
            // limit is closed, like iroh's own server does.
            tokio::select! {
                res = conn => if let Err(err) = res { debug!("{from}: {err}") },
                _ = established.notified() => {}
                _ = tokio::time::sleep(ESTABLISH) => debug!("{from}: took too long"),
            }
        });
    }
}

/// The captive-portal check on plain HTTP: `/generate_204`, echoing iroh's
/// challenge header. Nothing else is served without TLS.
pub fn captive_portal() -> Router {
    Router::new().route(
        "/generate_204",
        get(|headers: HeaderMap| async move {
            let mut res = StatusCode::NO_CONTENT.into_response();
            if let Some(ch) = headers
                .get("x-iroh-challenge")
                .and_then(|v| v.to_str().ok())
                && !ch.is_empty()
                && ch.len() < 64
                && ch
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                && let Ok(v) = format!("response {ch}").parse()
            {
                res.headers_mut().insert("x-iroh-response", v);
            }
            res
        }),
    )
}

#[cfg(test)]
mod tests {
    //! A real iroh relay client, through our front door and gate.

    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use iroh_dns::dns::DnsResolver;
    use iroh_relay::{
        KeyCache,
        client::{Client, ClientBuilder},
        protos::relay::{ClientToRelayMsg, RelayToClientMsg},
        server::{Metrics, http_server::Handlers},
        tls::{CaTlsConfig, default_provider},
    };
    use zoomriya_keys::{Pass, SecretKey, seal};
    use zoomriya_proto::{RemovedDevice, RemovedDevices};
    use zoomriya_serve::{CertSource, now};

    use super::*;
    use crate::gate::{GateAccess, GateConfig};

    struct Running {
        url: String,
        root: rustls::pki_types::CertificateDer<'static>,
        gate: Arc<Gate>,
        _dir: tempfile::TempDir,
    }

    async fn start() -> Running {
        start_with(Routes::default()).await
    }

    async fn start_with(routes: Routes) -> Running {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let dir = tempfile::tempdir().expect("dir");
        let cert =
            rcgen::generate_simple_self_signed(vec!["localhost".into(), "turn.localhost".into()])
                .expect("cert");
        let (c, k) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
        std::fs::write(&c, cert.cert.pem()).expect("write");
        std::fs::write(&k, cert.signing_key.serialize_pem()).expect("write");
        let gate = Arc::new(Gate::new(GateConfig {
            clock_allowance: 600,
            longest_pass: 31 * 86_400,
        }));
        let relay = RelayService::new(
            Handlers::default(),
            HeaderMap::new(),
            None,
            KeyCache::new(1024),
            Arc::new(GateAccess(gate.clone())),
            Arc::new(Metrics::default()),
        );
        gate.set_clients(relay.clients().clone());
        let listener = TlsListener::bind(
            "127.0.0.1:0".parse().expect("addr"),
            &CertSource::Files { cert: c, key: k },
            &[b"http/1.1"],
        )
        .await
        .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let api = router(Api {
            gate: gate.clone(),
            enrolled: None,
            clock_allowance: 600,
            laptops: Arc::default(),
            turn: None,
        });
        tokio::spawn(serve(listener, relay, api, gate.meter.clone(), routes));
        Running {
            url: format!("https://localhost:{port}"),
            root: cert.cert.der().clone(),
            gate,
            _dir: dir,
        }
    }

    async fn connect(
        r: &Running,
        key: &SecretKey,
        token: Option<String>,
    ) -> Result<Client, String> {
        let url = &r.url;
        let mut b = ClientBuilder::new(
            url.parse::<iroh_base::RelayUrl>().expect("url"),
            key.clone(),
            DnsResolver::new(),
        )
        .tls_client_config(
            CaTlsConfig::custom_roots([r.root.clone()])
                .client_config(default_provider())
                .expect("TLS config"),
        );
        if let Some(t) = token {
            b = b.auth_token(t);
        }
        b.connect().await.map_err(|e| format!("{e:#}"))
    }

    fn pass(b: &SecretKey, link: &SecretKey) -> String {
        Pass {
            box_key: b.public(),
            link_key: link.public(),
            not_before: now() - 60,
            not_after: now() + 30 * 86_400,
        }
        .sign(b)
        .expect("signs")
    }

    /// The next datagram, skipping pings and status.
    async fn next_datagram(c: &mut Client) -> Option<Bytes> {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), c.next()).await {
                Ok(Some(Ok(RelayToClientMsg::Datagrams { datagrams, .. }))) => {
                    return Some(datagrams.contents);
                }
                Ok(Some(Ok(_))) => continue,
                _ => return None,
            }
        }
    }

    /// A program on this machine: answers what it got first, with a label.
    async fn program(label: &'static str) -> SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = l.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4];
                    if s.read_exact(&mut buf).await.is_ok() {
                        let _ = s.write_all(format!("{label}:").as_bytes()).await;
                        let _ = s.write_all(&buf).await;
                    }
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn other_names_on_port_443() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let connect = program("connect").await;
        let coturn = program("turn").await;
        let r = start_with(Routes {
            pass_through: vec![("connect.localhost".into(), connect)],
            turn: Some(("turn.localhost".into(), coturn)),
        })
        .await;
        let port: u16 = r
            .url
            .rsplit(':')
            .next()
            .expect("port")
            .parse()
            .expect("port");

        // Passed on unopened: the program gets the TLS hello itself.
        let mut raw = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("tcp");
        let hello = {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("versions")
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth();
            let mut c = rustls::ClientConnection::new(
                Arc::new(config),
                rustls::pki_types::ServerName::try_from("connect.localhost").expect("name"),
            )
            .expect("client");
            let mut out = Vec::new();
            c.write_tls(&mut out).expect("hello");
            out
        };
        raw.write_all(&hello).await.expect("send");
        let mut got = [0u8; 12];
        raw.read_exact(&mut got).await.expect("answer");
        assert_eq!(&got[..8], b"connect:");
        assert_eq!(&got[8..], &hello[..4], "the hello arrived unopened");

        // TURN: TLS opened by the relay, the stream inside goes to coturn.
        let mut roots = rustls::RootCertStore::empty();
        roots.add(r.root.clone()).expect("root");
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("tcp");
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(
                rustls::pki_types::ServerName::try_from("turn.localhost").expect("name"),
                tcp,
            )
            .await
            .expect("TLS");
        tls.write_all(b"ping").await.expect("send");
        let mut got = [0u8; 9];
        tls.read_exact(&mut got).await.expect("answer");
        assert_eq!(&got, b"turn:ping");

        // Everything else is still the relay.
        let http = reqwest::Client::builder()
            .tls_certs_only([reqwest::Certificate::from_der(&r.root).expect("root")])
            .build()
            .expect("client");
        let res = http
            .get(format!("{}/healthz", r.url))
            .send()
            .await
            .expect("get");
        assert_eq!(res.status(), 200);
    }

    #[tokio::test]
    async fn laptops_offers_from_away() {
        use zoomriya_keys::sign_request;
        use zoomriya_proto::laptop::{
            LaptopOffer, RELAY_ANSWER, RELAY_CONNECT, RELAY_WAIT, RelayedAnswer, RelayedOffers,
        };
        let r = start().await;
        let b = SecretKey::generate();
        let laptop = SecretKey::generate();
        r.gate.apply(b.public(), BoxState::Active);
        let http = reqwest::Client::builder()
            .tls_certs_only([reqwest::Certificate::from_der(&r.root).expect("root")])
            .build()
            .expect("client");
        let offer = serde_json::to_vec(&LaptopOffer {
            link: laptop.public().into(),
            time: now(),
            sdp: "v=0".into(),
            sig: "x".into(),
        })
        .expect("json");
        let send_offer = |pass: String, signer: SecretKey| {
            let http = http.clone();
            let url = format!("{}{RELAY_CONNECT}", r.url);
            let offer = offer.clone();
            async move {
                http.post(url)
                    .header("authorization", format!("Bearer {pass}"))
                    .header(
                        REQUEST_SIGNATURE_HEADER,
                        sign_request(&signer, "POST", RELAY_CONNECT, now(), &offer),
                    )
                    .body(offer)
                    .send()
                    .await
                    .expect("post")
            }
        };

        // The box isn't listening yet.
        let res = send_offer(pass(&b, &laptop), laptop.clone()).await;
        assert_eq!(res.status(), 404);

        // The box waits; the laptop's offer reaches it; its answer comes back.
        let wait = {
            let http = http.clone();
            let url = format!("{}{RELAY_WAIT}", r.url);
            let sig = sign_request(&b, "POST", RELAY_WAIT, now(), b"");
            tokio::spawn(async move {
                let res = http
                    .post(url)
                    .header(REQUEST_SIGNATURE_HEADER, sig)
                    .send()
                    .await
                    .expect("wait");
                assert_eq!(res.status(), 200);
                res.json::<RelayedOffers>().await.expect("offers")
            })
        };
        tokio::time::sleep(Duration::from_millis(200)).await;
        let asking = tokio::spawn(send_offer(pass(&b, &laptop), laptop.clone()));
        let got = wait.await.expect("wait");
        assert_eq!(got.offers.len(), 1);
        assert_eq!(got.offers[0].offer.link.0, laptop.public());
        let answer = serde_json::to_vec(&RelayedAnswer {
            id: got.offers[0].id,
            answer: Some("sealed".into()),
            error: None,
        })
        .expect("json");
        let res = http
            .post(format!("{}{RELAY_ANSWER}", r.url))
            .header(
                REQUEST_SIGNATURE_HEADER,
                sign_request(&b, "POST", RELAY_ANSWER, now(), &answer),
            )
            .body(answer)
            .send()
            .await
            .expect("answer");
        assert_eq!(res.status(), 204);
        let res = asking.await.expect("asked");
        assert_eq!(res.status(), 200);
        assert_eq!(
            res.headers()
                .get("access-control-allow-origin")
                .map(|v| v.as_bytes()),
            Some(&b"*"[..])
        );
        let body: serde_json::Value = res.json().await.expect("json");
        assert_eq!(body["answer"], "sealed");

        // A pass shown by someone without its link key; and a stranger's box.
        let stranger = SecretKey::generate();
        let res = send_offer(pass(&b, &laptop), stranger.clone()).await;
        assert_eq!(res.status(), 403);
        let sig = sign_request(&stranger, "POST", RELAY_WAIT, now(), b"");
        let res = http
            .post(format!("{}{RELAY_WAIT}", r.url))
            .header(REQUEST_SIGNATURE_HEADER, sig)
            .send()
            .await
            .expect("wait");
        assert_eq!(res.status(), 403);
    }

    #[tokio::test]
    async fn passes_at_the_relay() {
        let r = start().await;
        let b = SecretKey::generate();
        let phone = SecretKey::generate();
        let stranger = SecretKey::generate();

        // Nobody's registered yet.
        assert!(connect(&r, &b, None).await.is_err());
        r.gate.apply(b.public(), BoxState::Active);

        let mut the_box = connect(&r, &b, None).await.expect("box connects");
        assert!(connect(&r, &stranger, None).await.is_err(), "no pass");
        assert!(
            connect(&r, &stranger, Some(pass(&b, &phone)))
                .await
                .is_err(),
            "someone else's pass"
        );
        let mut p = connect(&r, &phone, Some(pass(&b, &phone)))
            .await
            .expect("phone with a pass connects");

        // Data flows, phone to box.
        p.send(ClientToRelayMsg::Datagrams {
            dst_endpoint_id: b.public(),
            datagrams: b"hello box".into(),
        })
        .await
        .expect("send");
        assert_eq!(
            next_datagram(&mut the_box).await.as_deref(),
            Some(&b"hello box"[..])
        );

        // Counted on the box's own connection, and listed as connected.
        let counted = r.gate.meter.take();
        assert_eq!(counted.len(), 1, "only the box is counted");
        assert_eq!(counted[0].0, b.public());
        assert!(counted[0].1 >= 9, "at least the datagram itself");
        assert_eq!(r.gate.connected_boxes(), vec![(b.public(), 1)]);

        // The box removes the phone: its live connection is dropped, and it
        // can't come back with the same pass.
        let list = RemovedDevices {
            box_key: b.public().into(),
            issued: now(),
            full: true,
            devices: vec![RemovedDevice {
                link: phone.public().into(),
                until: now() + 30 * 86_400,
            }],
        };
        let body = seal(&b, zoomriya_keys::Purpose::RemovedDevices, &list);
        let http = reqwest::Client::builder()
            .tls_certs_only([reqwest::Certificate::from_der(&r.root).expect("root")])
            .build()
            .expect("client");
        let res = http
            .post(format!("{}/zoomriya/v1/removed", r.url))
            .body(body.clone())
            .send()
            .await
            .expect("post");
        assert_eq!(res.status(), 200);
        assert_eq!(next_datagram(&mut p).await, None, "dropped");
        assert!(connect(&r, &phone, Some(pass(&b, &phone))).await.is_err());

        // Sending the same list again is a replay.
        let res = http
            .post(format!("{}/zoomriya/v1/removed", r.url))
            .body(body)
            .send()
            .await
            .expect("post");
        assert_eq!(res.status(), 409);

        // A list signed by someone else is refused.
        let forged = seal(
            &stranger,
            zoomriya_keys::Purpose::RemovedDevices,
            &RemovedDevices {
                issued: now() + 1,
                full: true,
                devices: vec![],
                ..list
            },
        );
        let res = http
            .post(format!("{}/zoomriya/v1/removed", r.url))
            .body(forged)
            .send()
            .await
            .expect("post");
        assert_eq!(res.status(), 400);

        // Marking the box stolen drops the box too.
        r.gate.apply(b.public(), BoxState::Stolen);
        assert_eq!(next_datagram(&mut the_box).await, None, "box dropped");
        assert!(connect(&r, &b, None).await.is_err());

        // The probe and health answer.
        let res = http
            .get(format!("{}/ping", r.url))
            .send()
            .await
            .expect("ping");
        assert_eq!(res.status(), 200);
        let res = http
            .get(format!("{}/healthz", r.url))
            .send()
            .await
            .expect("health");
        assert_eq!(res.status(), 200);
    }
}
