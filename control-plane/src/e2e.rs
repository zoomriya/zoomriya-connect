//! The control plane end to end: real HTTP, real UDP DNS, a box's signed requests.

use std::{net::SocketAddr, sync::Arc};

use hickory_proto::{
    op::{Message, MessageType, OpCode, ResponseCode},
    rr::{Name, RData, RecordType},
};
use iroh_dns::pkarr::SignedPacket;
use reqwest_shim::Client;
use zoomriya_keys::{REQUEST_SIGNATURE_HEADER, SecretKey, sign_request};
use zoomriya_proto::{
    AcmeRequest, AcmeTarget, BoxFeed, BoxState, ErrorBody, HomeAddrs, PROTOCOL_VERSION,
    RegisterRequest, RegisterResponse, VERSION_HEADER, WellKnown,
};
use zoomriya_serve::now;

use crate::{config::Config, store::Store};

/// A tiny HTTP client over hyper, so the control plane needs no HTTP client of its own.
mod reqwest_shim {
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    use hyper_util::{client::legacy::Client as Inner, rt::TokioExecutor};

    pub struct Client(Inner<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>);

    pub struct Response {
        pub status: u16,
        pub body: Vec<u8>,
    }

    impl Client {
        pub fn new() -> Self {
            Self(Inner::builder(TokioExecutor::new()).build_http())
        }

        pub async fn send(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, String)],
            body: Vec<u8>,
        ) -> Response {
            let mut req = hyper::Request::builder().method(method).uri(url);
            for (k, v) in headers {
                req = req.header(*k, v);
            }
            let res = self
                .0
                .request(req.body(Full::new(Bytes::from(body))).expect("request"))
                .await
                .expect("send");
            let status = res.status().as_u16();
            let body = res
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes()
                .to_vec();
            Response { status, body }
        }
    }
}

struct Running {
    http: String,
    admin: String,
    admin_token: String,
    dns: SocketAddr,
    store: Arc<Store>,
    client: Client,
    _dir: tempfile::TempDir,
}

const RELAY_KEY: &str = "relay-key-0123456789abcdefghijklmnopqrstuvwxyz";
const TURN_SECRET: &str = "turn-secret-0123456789abcdefghijklmnopqrstuvwxyz";
const ZONE_KEY: &str = "zone-key-0123456789abcdefghijklmnopqrstuvwxyz";
const ACCOUNT: &str = "smiths";
const ACCOUNT_PASSWORD: &str = "abcd-efgh-jkmn-pqrs";

fn login() -> Option<zoomriya_proto::AccountLogin> {
    Some(zoomriya_proto::AccountLogin {
        username: "Smiths".into(),
        password: ACCOUNT_PASSWORD.into(),
    })
}

async fn start() -> Running {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().expect("dir");
    let keys = dir.path().join("relay-keys");
    std::fs::write(&keys, format!("# old\n{RELAY_KEY}\n")).expect("write");
    let words = dir.path().join("blocked-words");
    std::fs::write(&words, "acme\n").expect("write");
    let turn = dir.path().join("turn-secrets");
    std::fs::write(&turn, format!("{TURN_SECRET}\n")).expect("write");
    let zone_keys = dir.path().join("zone-keys");
    std::fs::write(&zone_keys, format!("{ZONE_KEY}\n")).expect("write");
    let web = dir.path().join("web");
    std::fs::create_dir_all(&web).expect("web");
    std::fs::write(web.join("index.html"), "<!doctype html><title>GUI</title>").expect("write");
    let text = include_str!("../../deploy/control-plane.toml")
        .replace(
            "/var/lib/zoomriya-control-plane/control-plane.sqlite",
            &dir.path().join("db.sqlite").display().to_string(),
        )
        .replace(
            "/etc/zoomriya-control-plane/relay-keys",
            &keys.display().to_string(),
        )
        .replace(
            "/etc/zoomriya-control-plane/blocked-words",
            &words.display().to_string(),
        )
        .replace(
            "/etc/zoomriya-control-plane/turn-secrets",
            &turn.display().to_string(),
        )
        .replace(
            "/etc/zoomriya-control-plane/zone-keys",
            &zone_keys.display().to_string(),
        )
        .replace(
            "/var/lib/zoomriya-control-plane/admin-tokens",
            &dir.path().join("admin-tokens").display().to_string(),
        )
        .replace(
            "/usr/share/zoomriya-connect/admin",
            &web.display().to_string(),
        );
    let cfg: Config = toml::from_str(&text).expect("config");
    let store = Arc::new(Store::open(&cfg.database).expect("store"));
    // An account an operator made for the tests' family, for up to 10 boxes.
    store
        .create_account(
            ACCOUNT,
            &crate::accounts::hash(ACCOUNT_PASSWORD).expect("hash"),
            10,
            now(),
        )
        .expect("account");
    let (api, dns) = crate::build(&cfg, store.clone()).expect("build");
    let dns = crate::dns::serve("127.0.0.1:0".parse().expect("addr"), dns)
        .await
        .expect("dns");
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let http = format!("http://{}", l.local_addr().expect("addr"));
    tokio::spawn(zoomriya_serve::serve_plain(
        l,
        crate::api::router(api.clone()),
    ));
    // The GUI's API.
    let admin_cfg = cfg.admin.clone().expect("admin");
    let admin_token =
        zoomriya_serve::admin::new_token(&admin_cfg.tokens_file, "tester").expect("token");
    let routes = crate::admin_api::router(Arc::new(crate::admin_api::Admin {
        api,
        relays: Default::default(),
    }));
    let router = zoomriya_serve::admin::router(
        routes,
        zoomriya_serve::admin::Tokens::new(admin_cfg.tokens_file.clone()),
        &admin_cfg.web_dir,
    );
    let al = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let admin = format!("http://{}", al.local_addr().expect("addr"));
    tokio::spawn(zoomriya_serve::serve_plain(al, router));
    Running {
        http,
        admin,
        admin_token,
        dns,
        store,
        client: Client::new(),
        _dir: dir,
    }
}

impl Running {
    async fn signed(
        &self,
        key: &SecretKey,
        method: &str,
        path: &str,
        body: &impl serde::Serialize,
    ) -> reqwest_shim::Response {
        let body = serde_json::to_vec(body).expect("json");
        let sig = sign_request(key, method, path, now(), &body);
        self.client
            .send(
                method,
                &format!("{}{path}", self.http),
                &[
                    (REQUEST_SIGNATURE_HEADER, sig),
                    (VERSION_HEADER, PROTOCOL_VERSION.to_string()),
                    ("content-type", "application/json".into()),
                ],
                body,
            )
            .await
    }

    async fn dns(&self, name: &str, qtype: RecordType) -> Message {
        let mut q = Message::new(7, MessageType::Query, OpCode::Query);
        q.add_query(hickory_proto::op::Query::query(
            Name::from_ascii(name).expect("name"),
            qtype,
        ));
        let sock = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("udp");
        sock.send_to(&q.to_vec().expect("encode"), self.dns)
            .await
            .expect("send");
        let mut buf = [0u8; 2048];
        let n = tokio::time::timeout(std::time::Duration::from_secs(3), sock.recv(&mut buf))
            .await
            .expect("answer in time")
            .expect("recv");
        Message::from_vec(&buf[..n]).expect("decode")
    }
}

fn code(body: &[u8]) -> String {
    serde_json::from_slice::<ErrorBody>(body)
        .map(|e| e.code)
        .unwrap_or_else(|_| String::from_utf8_lossy(body).into_owned())
}

#[tokio::test]
async fn a_box_from_registration_to_dns() {
    let r = start().await;
    let box_key = SecretKey::generate();
    let dns_key = SecretKey::generate();

    // What a box reads first.
    let wk = r
        .client
        .send(
            "GET",
            &format!("{}/.well-known/zoomriya-connect.json", r.http),
            &[],
            vec![],
        )
        .await;
    assert_eq!(wk.status, 200);
    let wk: WellKnown = serde_json::from_slice(&wk.body).expect("well-known");
    assert_eq!(wk.zone, "zoomriya.app");
    assert_eq!(wk.address_records, "https://connect.zoomriya.net/pkarr");

    let reg = RegisterRequest {
        name: "Maple".into(),
        box_key: box_key.public().into(),
        dns_update_key: dns_key.public().into(),
        home: HomeAddrs {
            ipv4: vec!["192.168.1.20".parse().expect("ip")],
            ipv6: vec![],
        },
        factory_token: None,
        account: None,
    };

    // Unsigned, signed by the wrong key, an old version.
    let res = r
        .client
        .send(
            "POST",
            &format!("{}/v1/boxes", r.http),
            &[(VERSION_HEADER, "1".into())],
            serde_json::to_vec(&reg).expect("json"),
        )
        .await;
    assert_eq!((res.status, code(&res.body).as_str()), (401, "unsigned"));
    let res = r.signed(&dns_key, "POST", "/v1/boxes", &reg).await;
    assert_eq!(res.status, 401);
    let res = r
        .client
        .send(
            "POST",
            &format!("{}/v1/boxes", r.http),
            &[(VERSION_HEADER, "0".into())],
            vec![],
        )
        .await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (426, "please_update")
    );
    // The operators' extra words.
    let res = r
        .signed(
            &box_key,
            "POST",
            "/v1/boxes",
            &RegisterRequest {
                name: "acme-home".into(),
                ..reg.clone()
            },
        )
        .await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (400, "name_blocked")
    );

    // No account, or the wrong password: refused.
    let res = r.signed(&box_key, "POST", "/v1/boxes", &reg).await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (403, "account_needed")
    );
    let mut wrong = reg.clone();
    wrong.account = Some(zoomriya_proto::AccountLogin {
        username: ACCOUNT.into(),
        password: "not-the-password".into(),
    });
    let res = r.signed(&box_key, "POST", "/v1/boxes", &wrong).await;
    assert_eq!((res.status, code(&res.body).as_str()), (401, "wrong_login"));

    // Registered, with the account.
    let mut reg = reg;
    reg.account = login();
    let res = r.signed(&box_key, "POST", "/v1/boxes", &reg).await;
    assert_eq!(res.status, 201, "{}", String::from_utf8_lossy(&res.body));
    // Again, as at each start: no password needed any more.
    reg.account = None;
    let ok: RegisterResponse = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(ok.fqdn, "maple.zoomriya.app");
    assert_eq!(ok.share_fqdn, "maple-share.zoomriya.app");

    // DNS answers for it.
    let m = r.dns("maple.zoomriya.app.", RecordType::A).await;
    assert_eq!(m.metadata.response_code, ResponseCode::NoError);
    assert!(m.metadata.authoritative);
    assert!(
        matches!(m.answers[0].data, RData::A(a) if a.0 == std::net::Ipv4Addr::new(192, 168, 1, 20))
    );
    let m = r.dns("oak.zoomriya.app.", RecordType::A).await;
    assert_eq!(m.metadata.response_code, ResponseCode::NXDomain);
    assert_eq!(m.authorities.len(), 1, "SOA for the negative answer");
    let m = r.dns("example.com.", RecordType::A).await;
    assert_eq!(m.metadata.response_code, ResponseCode::Refused);

    // Certificate check: only the DNS update key can.
    let acme = AcmeRequest {
        target: AcmeTarget::Box,
        values: vec!["check-123".into()],
    };
    let res = r
        .signed(&box_key, "PUT", "/v1/boxes/maple/acme", &acme)
        .await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (403, "not_your_box")
    );
    let res = r
        .signed(&dns_key, "PUT", "/v1/boxes/maple/acme", &acme)
        .await;
    assert_eq!(res.status, 204);
    let m = r
        .dns("_acme-challenge.maple.zoomriya.app.", RecordType::TXT)
        .await;
    assert_eq!(m.answers.len(), 1);
    assert!(m.answers[0].to_string().contains("check-123"));

    // The door can't point anywhere but home: here, the request comes from
    // 127.0.0.1, which isn't public.
    let res = r
        .signed(
            &dns_key,
            "PUT",
            "/v1/boxes/maple/door",
            &serde_json::json!({ "ipv4": "81.2.69.160" }),
        )
        .await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (403, "door_not_home")
    );

    // Address records: only registered boxes may publish.
    let packet = SignedPacket::from_txt_strings(
        &box_key,
        "_iroh",
        ["relay=https://relay1.zoomriya.net./"],
        30,
    )
    .expect("packet");
    let url = format!("{}/pkarr/{}", r.http, box_key.public().to_z32());
    let res = r
        .client
        .send("PUT", &url, &[], packet.to_relay_payload())
        .await;
    assert_eq!(res.status, 204, "{}", String::from_utf8_lossy(&res.body));
    let res = r
        .client
        .send("PUT", &url, &[], packet.to_relay_payload())
        .await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (409, "older_record")
    );
    let res = r.client.send("GET", &url, &[], vec![]).await;
    assert_eq!(res.body, packet.to_relay_payload());
    let stranger = SecretKey::generate();
    let theirs =
        SignedPacket::from_txt_strings(&stranger, "_iroh", ["relay=https://evil.example./"], 30)
            .expect("packet");
    let res = r
        .client
        .send(
            "PUT",
            &format!("{}/pkarr/{}", r.http, stranger.public().to_z32()),
            &[],
            theirs.to_relay_payload(),
        )
        .await;
    assert_eq!((res.status, code(&res.body).as_str()), (403, "unknown_box"));
    // Signed by someone else for the box's key.
    let res = r
        .client
        .send("PUT", &url, &[], theirs.to_relay_payload())
        .await;
    assert_eq!((res.status, code(&res.body).as_str()), (400, "bad_record"));
    let m = r
        .dns(
            &format!("_iroh.{}.dns.zoomriya.net.", box_key.public().to_z32()),
            RecordType::TXT,
        )
        .await;
    assert_eq!(m.answers.len(), 1);

    // Relays: the list of boxes, never devices.
    let res = r
        .client
        .send(
            "GET",
            &format!("{}/v1/relay/boxes?after=0", r.http),
            &[],
            vec![],
        )
        .await;
    assert_eq!(res.status, 401);
    let res = r
        .client
        .send(
            "GET",
            &format!("{}/v1/relay/boxes?after=0", r.http),
            &[("authorization", format!("Bearer {RELAY_KEY}"))],
            vec![],
        )
        .await;
    assert_eq!(res.status, 200);
    let feed: BoxFeed = serde_json::from_slice(&res.body).expect("feed");
    assert!(feed.full);
    assert_eq!(feed.changes.len(), 1);
    assert_eq!(feed.changes[0].key.0, box_key.public());
    assert_eq!(feed.changes[0].state, BoxState::Active);

    // Switched off by an operator, in another process: the name stops answering.
    let other = Store::open(&r._dir.path().join("db.sqlite")).expect("second handle");
    assert!(other.set_disabled("maple", true).expect("off"));
    assert!(r.store.reload_if_changed().expect("reload"));
    let m = r.dns("maple.zoomriya.app.", RecordType::A).await;
    assert_eq!(m.metadata.response_code, ResponseCode::NXDomain);

    // Releasing by the box key only.
    assert!(other.set_disabled("maple", false).expect("on"));
    r.store.reload_if_changed().expect("reload");
    let res = r
        .signed(
            &dns_key,
            "POST",
            "/v1/boxes/maple/release",
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(res.status, 403);
    let res = r
        .signed(
            &box_key,
            "POST",
            "/v1/boxes/maple/release",
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(res.status, 204);
    let feed: BoxFeed = serde_json::from_slice(
        &r.client
            .send(
                "GET",
                &format!("{}/v1/relay/boxes?after={}", r.http, feed.seq),
                &[("authorization", format!("Bearer {RELAY_KEY}"))],
                vec![],
            )
            .await
            .body,
    )
    .expect("feed");
    assert_eq!(feed.changes.last().map(|c| c.state), Some(BoxState::Gone));
}

async fn registered(r: &Running, name: &str) -> (SecretKey, SecretKey) {
    let box_key = SecretKey::generate();
    let dns_key = SecretKey::generate();
    let reg = RegisterRequest {
        name: name.into(),
        box_key: box_key.public().into(),
        dns_update_key: dns_key.public().into(),
        home: HomeAddrs {
            ipv4: vec!["192.168.1.20".parse().expect("ip")],
            ipv6: vec![],
        },
        factory_token: None,
        account: login(),
    };
    let res = r.signed(&box_key, "POST", "/v1/boxes", &reg).await;
    assert_eq!(res.status, 201, "{}", String::from_utf8_lossy(&res.body));
    (box_key, dns_key)
}

#[tokio::test]
async fn admin_notes() {
    use zoomriya_keys::{Purpose, seal};
    use zoomriya_proto::{AdminNote, MoveRequest, StolenRequest};
    let r = start().await;
    let (old_box, _) = registered(&r, "maple").await;
    let admin = SecretKey::generate();
    let note = seal(
        &old_box,
        Purpose::AdminNote,
        &AdminNote {
            box_key: old_box.public().into(),
            admin: admin.public().into(),
            issued: now(),
        },
    );
    // Someone else showing the admin's note.
    let thief = SecretKey::generate();
    let new_box = SecretKey::generate();
    let mv = MoveRequest {
        note: note.clone(),
        new_box_key: new_box.public().into(),
        new_dns_update_key: SecretKey::generate().public().into(),
    };
    let res = r.signed(&thief, "POST", "/v1/admin/move", &mv).await;
    assert_eq!((res.status, code(&res.body).as_str()), (403, "bad_note"));
    // A note the admin signed for themselves, not the box.
    let fake = seal(
        &admin,
        Purpose::AdminNote,
        &AdminNote {
            box_key: old_box.public().into(),
            admin: admin.public().into(),
            issued: now(),
        },
    );
    let res = r
        .signed(
            &admin,
            "POST",
            "/v1/admin/stolen",
            &StolenRequest { note: fake },
        )
        .await;
    assert_eq!((res.status, code(&res.body).as_str()), (403, "bad_note"));
    // The admin moves the name.
    let res = r.signed(&admin, "POST", "/v1/admin/move", &mv).await;
    assert_eq!(res.status, 200, "{}", String::from_utf8_lossy(&res.body));
    assert_eq!(
        r.store.find("maple").expect("db").expect("row").box_key,
        new_box.public()
    );
    // The old box's note can't do anything more.
    let res = r
        .signed(&admin, "POST", "/v1/admin/stolen", &StolenRequest { note })
        .await;
    assert_eq!(res.status, 404);
    // A note from the new box marks it stolen.
    let note = seal(
        &new_box,
        Purpose::AdminNote,
        &AdminNote {
            box_key: new_box.public().into(),
            admin: admin.public().into(),
            issued: now(),
        },
    );
    let res = r
        .signed(&admin, "POST", "/v1/admin/stolen", &StolenRequest { note })
        .await;
    assert_eq!(res.status, 204);
    assert!(r.store.find("maple").expect("db").expect("row").stolen);
}

#[tokio::test]
async fn guests_relay_credentials() {
    use zoomriya_proto::TurnCredentials;
    let r = start().await;
    let (box_key, dns_key) = registered(&r, "maple").await;
    let res = r
        .signed(
            &dns_key,
            "POST",
            "/v1/boxes/maple/turn",
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(res.status, 403, "only the box key");
    let res = r
        .signed(
            &box_key,
            "POST",
            "/v1/boxes/maple/turn",
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(res.status, 200);
    let c: TurnCredentials = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(c.ttl, 3600);
    assert!(c.urls[0].starts_with("turn:"));
}

#[tokio::test]
async fn guests_setup_messages() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message as Ws, client::IntoClientRequest};
    use zoomriya_proto::{SignalEvent, SignalMessage};
    let r = start().await;
    let (box_key, _) = registered(&r, "maple").await;
    let ws = r.http.replace("http://", "ws://");

    // A guest before the box is there.
    let mut req = format!("{ws}/v1/signal/guest/maple")
        .into_client_request()
        .expect("req");
    req.headers_mut()
        .insert("origin", "https://share.zoomriya.net".parse().expect("h"));
    assert!(
        tokio_tungstenite::connect_async(req).await.is_err(),
        "box offline"
    );

    // The box connects, signed.
    let mut req = format!("{ws}/v1/signal/box")
        .into_client_request()
        .expect("req");
    let sig = sign_request(&box_key, "GET", "/v1/signal/box", now(), b"");
    req.headers_mut()
        .insert(REQUEST_SIGNATURE_HEADER, sig.parse().expect("h"));
    req.headers_mut()
        .insert(VERSION_HEADER, "1".parse().expect("h"));
    let (mut the_box, _) = tokio_tungstenite::connect_async(req)
        .await
        .expect("box connects");

    // A page elsewhere can't open a guest socket.
    let mut req = format!("{ws}/v1/signal/guest/maple")
        .into_client_request()
        .expect("req");
    req.headers_mut()
        .insert("origin", "https://evil.example".parse().expect("h"));
    assert!(
        tokio_tungstenite::connect_async(req).await.is_err(),
        "wrong origin"
    );

    let mut req = format!("{ws}/v1/signal/guest/maple")
        .into_client_request()
        .expect("req");
    req.headers_mut()
        .insert("origin", "https://share.zoomriya.net".parse().expect("h"));
    let (mut guest, _) = tokio_tungstenite::connect_async(req)
        .await
        .expect("guest connects");
    guest
        .send(Ws::Text("offer-sdp".into()))
        .await
        .expect("send");

    async fn next_event<S>(s: &mut S) -> SignalEvent
    where
        S: futures_util::Stream<Item = Result<Ws, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        loop {
            match s.next().await {
                Some(Ok(Ws::Text(t))) => return serde_json::from_str(&t).expect("event"),
                Some(Ok(Ws::Ping(_) | Ws::Pong(_))) => continue,
                other => panic!("{other:?}"),
            }
        }
    }
    let ev = next_event(&mut the_box).await;
    let SignalEvent::Message(SignalMessage { session, data }) = ev else {
        panic!("{ev:?}")
    };
    assert_eq!(data, "offer-sdp");
    let answer = serde_json::to_string(&SignalMessage {
        session,
        data: "answer-sdp".into(),
    })
    .expect("json");
    the_box.send(Ws::Text(answer.into())).await.expect("send");
    match guest.next().await {
        Some(Ok(Ws::Text(t))) => assert_eq!(t.as_str(), "answer-sdp"),
        other => panic!("{other:?}"),
    }
    drop(guest);
    assert_eq!(
        next_event(&mut the_box).await,
        SignalEvent::Gone { session }
    );
}

#[tokio::test]
async fn second_dns_server_copies() {
    let r = start().await;
    registered(&r, "maple").await;
    let res = r
        .client
        .send("GET", &format!("{}/v1/zone", r.http), &[], vec![])
        .await;
    assert_eq!(res.status, 401);
    let auth = ("authorization", format!("Bearer {ZONE_KEY}"));
    let res = r
        .client
        .send(
            "GET",
            &format!("{}/v1/zone", r.http),
            std::slice::from_ref(&auth),
            vec![],
        )
        .await;
    assert_eq!(res.status, 200);
    let etag = format!("\"{}\"", r.store.serial());
    let res = r
        .client
        .send(
            "GET",
            &format!("{}/v1/zone", r.http),
            &[auth, ("if-none-match", etag)],
            vec![],
        )
        .await;
    assert_eq!(res.status, 304);

    // The second server follows, and answers DNS from its copy.
    let copy = Arc::new(Store::open_memory().expect("store"));
    tokio::spawn(crate::zone::follow(
        copy.clone(),
        r.http.clone(),
        ZONE_KEY.into(),
        std::time::Duration::from_millis(100),
    ));
    for _ in 0..50 {
        if copy.entry("maple").is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(copy.entry("maple"), r.store.entry("maple"));
}

#[tokio::test]
async fn the_gui_api() {
    let r = start().await;
    registered(&r, "maple").await;
    let r = &r;
    let get = |path: &str, token: Option<&str>| {
        let url = format!("{}{path}", r.admin);
        let headers: Vec<(&str, String)> = token
            .map(|t| vec![("authorization", format!("Bearer {t}"))])
            .unwrap_or_default();
        async move { r.client.send("GET", &url, &headers, vec![]).await }
    };
    assert_eq!(get("/admin/api/overview", None).await.status, 401);
    assert_eq!(
        get("/admin/api/overview", Some("zat_wrong")).await.status,
        401
    );
    let res = get("/admin/api/overview", Some(&r.admin_token)).await;
    assert_eq!(res.status, 200);
    let o: serde_json::Value = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(o["boxes"]["all"], 1);
    assert_eq!(o["kind"], "control_plane");
    let res = get("/admin/api/boxes?q=map", Some(&r.admin_token)).await;
    let list: serde_json::Value = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(list[0]["name"], "maple");
    let res = r
        .client
        .send(
            "POST",
            &format!("{}/admin/api/boxes/maple/switch-off", r.admin),
            &[("authorization", format!("Bearer {}", r.admin_token))],
            vec![],
        )
        .await;
    assert_eq!(res.status, 204);
    assert!(r.store.find("maple").expect("db").expect("row").disabled);
    // The GUI's page, with its security headers; any other path gets the page too.
    let res = get("/boxes", None).await;
    assert_eq!(res.status, 200);
    assert!(String::from_utf8_lossy(&res.body).contains("GUI"));
}

#[tokio::test]
async fn monthly_allowance_end_to_end() {
    use zoomriya_proto::{BoxConnected, BoxUse, RelayReport, RelayReportAnswer, RelayUse};
    let r = start().await;
    let (maple, _) = registered(&r, "maple").await;
    let auth = |t: &str| vec![("authorization", format!("Bearer {t}"))];
    let json = ("content-type", "application/json".to_string());

    // A relay reports 3 GB for the box, which has a phone connected.
    let report = RelayReport {
        relay: "relay1.zoomriya.net".into(),
        used: vec![BoxUse {
            key: maple.public().into(),
            bytes: 3_000_000_001,
        }],
        connected: vec![BoxConnected {
            key: maple.public().into(),
            devices: 1,
        }],
    };
    let body = serde_json::to_vec(&report).expect("json");
    let url = format!("{}/v1/relay/report", r.http);
    let res = r
        .client
        .send("POST", &url, &auth("not a relay key at all"), body.clone())
        .await;
    assert_eq!(res.status, 401);
    let mut h = auth(RELAY_KEY);
    h.push(json.clone());
    let res = r.client.send("POST", &url, &h, body.clone()).await;
    assert_eq!(res.status, 200, "{}", String::from_utf8_lossy(&res.body));
    let a: RelayReportAnswer = serde_json::from_slice(&res.body).expect("answer");
    assert!(a.slow.is_empty(), "no limit until an operator sets one");

    // The operator sets 3 GB in the GUI: the next report slows the box.
    let mut h_admin = auth(&r.admin_token);
    h_admin.push(json.clone());
    let res = r
        .client
        .send(
            "PUT",
            &format!("{}/admin/api/allowances", r.admin),
            &h_admin,
            br#"{"default_gb":3,"factory_gb":50,"slow_kbps":125}"#.to_vec(),
        )
        .await;
    assert_eq!(res.status, 204);
    let empty = RelayReport {
        relay: "relay1.zoomriya.net".into(),
        used: vec![],
        connected: report.connected.clone(),
    };
    let res = r
        .client
        .send("POST", &url, &h, serde_json::to_vec(&empty).expect("json"))
        .await;
    let a: RelayReportAnswer = serde_json::from_slice(&res.body).expect("answer");
    assert_eq!(a.slow, vec![maple.public().into()]);
    assert_eq!(a.slow_bytes_per_second, 125_000);

    // The box sees it, for Box Desktop.
    let res = r
        .signed(
            &maple,
            "POST",
            "/v1/boxes/maple/relay-use",
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(res.status, 200, "{}", String::from_utf8_lossy(&res.body));
    let u: RelayUse = serde_json::from_slice(&res.body).expect("use");
    assert_eq!(u.used, 3_000_000_001);
    assert_eq!(u.allowance, 3_000_000_000);
    assert!(u.slowed);
    // Another box can't ask about this one.
    let other = SecretKey::generate();
    let res = r
        .signed(
            &other,
            "POST",
            "/v1/boxes/maple/relay-use",
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(res.status, 403);

    // The GUI: who's connected, and the box's line.
    let res = r
        .client
        .send(
            "GET",
            &format!("{}/admin/api/connected", r.admin),
            &auth(&r.admin_token),
            vec![],
        )
        .await;
    let c: serde_json::Value = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(c["boxes"][0]["name"], "maple");
    assert_eq!(c["boxes"][0]["devices"], 1);
    assert_eq!(c["boxes"][0]["relay"], "relay1.zoomriya.net");
    let res = r
        .client
        .send(
            "GET",
            &format!("{}/admin/api/boxes", r.admin),
            &auth(&r.admin_token),
            vec![],
        )
        .await;
    let list: serde_json::Value = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(list[0]["slowed"], true);
    assert_eq!(list[0]["allowance_gb"], 3);
    assert_eq!(list[0]["relay"], "relay1.zoomriya.net");

    // A bigger allowance for this box alone lifts it.
    let res = r
        .client
        .send(
            "POST",
            &format!("{}/admin/api/boxes/maple/allowance", r.admin),
            &h_admin,
            br#"{"gb":10}"#.to_vec(),
        )
        .await;
    assert_eq!(res.status, 204);
    let res = r
        .client
        .send("POST", &url, &h, serde_json::to_vec(&empty).expect("json"))
        .await;
    let a: RelayReportAnswer = serde_json::from_slice(&res.body).expect("answer");
    assert!(a.slow.is_empty());
    let res = r
        .client
        .send(
            "GET",
            &format!("{}/admin/api/overview", r.admin),
            &auth(&r.admin_token),
            vec![],
        )
        .await;
    let o: serde_json::Value = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(o["connected"]["boxes"], 1);
    assert_eq!(o["relay_use"]["bytes"], 3_000_000_001u64);
}

#[tokio::test]
async fn accounts_end_to_end() {
    let r = start().await;
    let r = &r;
    let admin = |method: &'static str, path: String, body: &'static str| async move {
        let mut h = vec![("authorization", format!("Bearer {}", r.admin_token))];
        if !body.is_empty() {
            h.push(("content-type", "application/json".into()));
        }
        r.client
            .send(
                method,
                &format!("{}/admin/api{path}", r.admin),
                &h,
                body.as_bytes().to_vec(),
            )
            .await
    };
    let json = |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).expect("json");

    // An operator makes an account for one box; its password is shown once.
    let res = admin(
        "POST",
        "/accounts".into(),
        r#"{"username":"Jones","max_boxes":1}"#,
    )
    .await;
    assert_eq!(res.status, 200, "{}", String::from_utf8_lossy(&res.body));
    let made = json(&res.body);
    assert_eq!(made["username"], "jones");
    let password = made["password"].as_str().expect("password").to_string();
    let res = admin("POST", "/accounts".into(), r#"{"username":"jones"}"#).await;
    assert_eq!(res.status, 409, "taken");

    // The family links a box with it; a second box doesn't fit.
    let box_key = SecretKey::generate();
    let reg = |name: &str, key: &SecretKey, pw: &str| RegisterRequest {
        name: name.into(),
        box_key: key.public().into(),
        dns_update_key: SecretKey::generate().public().into(),
        home: HomeAddrs {
            ipv4: vec!["192.168.1.20".parse().expect("ip")],
            ipv6: vec![],
        },
        factory_token: None,
        account: Some(zoomriya_proto::AccountLogin {
            username: "jones".into(),
            password: pw.into(),
        }),
    };
    let res = r
        .signed(
            &box_key,
            "POST",
            "/v1/boxes",
            &reg("oak", &box_key, &password),
        )
        .await;
    assert_eq!(res.status, 201, "{}", String::from_utf8_lossy(&res.body));
    let ok: RegisterResponse = serde_json::from_slice(&res.body).expect("json");
    assert_eq!(ok.account.as_deref(), Some("jones"));
    let other = SecretKey::generate();
    let res = r
        .signed(&other, "POST", "/v1/boxes", &reg("elm", &other, &password))
        .await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (403, "account_full")
    );

    // The account page: sign in, see the box, change the password.
    let page = |method: &'static str, path: &'static str, token: Option<String>, body: String| async move {
        let mut h = vec![("content-type", "application/json".to_string())];
        if let Some(t) = token {
            h.push(("authorization", format!("Bearer {t}")));
        }
        r.client
            .send(method, &format!("{}{path}", r.http), &h, body.into_bytes())
            .await
    };
    let sign_in = |pw: String| {
        page(
            "POST",
            "/v1/account/sign-in",
            None,
            serde_json::json!({ "username": "Jones", "password": pw }).to_string(),
        )
    };
    let res = sign_in(password.clone()).await;
    assert_eq!(res.status, 200, "{}", String::from_utf8_lossy(&res.body));
    let token = json(&res.body)["token"]
        .as_str()
        .expect("token")
        .to_string();
    let res = page("GET", "/v1/account", Some(token.clone()), String::new()).await;
    let me = json(&res.body);
    assert_eq!(me["username"], "jones");
    assert_eq!(me["boxes"][0]["name"], "oak");
    assert_eq!(me["boxes"][0]["fqdn"], "oak.zoomriya.app");
    let res = page(
        "POST",
        "/v1/account/password",
        Some(token.clone()),
        serde_json::json!({ "password": password, "new_password": "short" }).to_string(),
    )
    .await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (400, "password_short")
    );
    let res = page(
        "POST",
        "/v1/account/password",
        Some(token.clone()),
        serde_json::json!({ "password": password, "new_password": "our own long password" })
            .to_string(),
    )
    .await;
    assert_eq!(res.status, 204, "{}", String::from_utf8_lossy(&res.body));
    // Every sign-in ends; the old password no longer works, the new one does.
    let res = page("GET", "/v1/account", Some(token), String::new()).await;
    assert_eq!(res.status, 401);
    assert_eq!(sign_in(password.clone()).await.status, 401);
    assert_eq!(sign_in("our own long password".into()).await.status, 200);

    // Five wrong passwords lock it for a while, even with the right one.
    for _ in 0..5 {
        assert_eq!(sign_in("wrong password".into()).await.status, 401);
    }
    let res = sign_in("our own long password".into()).await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (429, "account_locked")
    );

    // The operator resets the password: that unlocks it too.
    let res = admin("POST", "/accounts/jones/reset-password".into(), "").await;
    assert_eq!(res.status, 200);
    let reset = json(&res.body)["password"]
        .as_str()
        .expect("password")
        .to_string();
    assert_eq!(sign_in(reset.clone()).await.status, 200);

    // Blocked: the box's name stops answering, relays are told it's gone,
    // registering again and signing in are refused.
    let feed = || async {
        let res = r
            .client
            .send(
                "GET",
                &format!("{}/v1/relay/boxes?after=0", r.http),
                &[("authorization", format!("Bearer {RELAY_KEY}"))],
                vec![],
            )
            .await;
        serde_json::from_slice::<BoxFeed>(&res.body).expect("feed")
    };
    assert!(
        feed()
            .await
            .changes
            .iter()
            .any(|c| c.key == box_key.public().into())
    );
    assert!(r.store.entry("oak").is_some());
    let res = admin("POST", "/accounts/jones/block".into(), "").await;
    assert_eq!(res.status, 204);
    assert!(r.store.entry("oak").is_none(), "out of DNS");
    assert!(
        !feed()
            .await
            .changes
            .iter()
            .any(|c| c.key == box_key.public().into()),
        "relays drop it"
    );
    let mut again = reg("oak", &box_key, "");
    again.account = None;
    let res = r.signed(&box_key, "POST", "/v1/boxes", &again).await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (403, "account_blocked")
    );
    let res = sign_in(reset.clone()).await;
    assert_eq!(
        (res.status, code(&res.body).as_str()),
        (403, "account_blocked")
    );

    // Unblocked: all back.
    assert_eq!(
        admin("POST", "/accounts/jones/unblock".into(), "")
            .await
            .status,
        204
    );
    assert!(r.store.entry("oak").is_some());
    assert!(
        feed()
            .await
            .changes
            .iter()
            .any(|c| c.key == box_key.public().into())
    );
    let res = r.signed(&box_key, "POST", "/v1/boxes", &again).await;
    assert_eq!(res.status, 200, "{}", String::from_utf8_lossy(&res.body));

    // Room for a second box.
    let res = admin(
        "POST",
        "/accounts/jones/max-boxes".into(),
        r#"{"max_boxes":2}"#,
    )
    .await;
    assert_eq!(res.status, 204);
    let res = r
        .signed(&other, "POST", "/v1/boxes", &reg("elm", &other, &reset))
        .await;
    assert_eq!(res.status, 201, "{}", String::from_utf8_lossy(&res.body));

    // The GUI's lists.
    let res = admin("GET", "/accounts?q=jon".into(), "").await;
    let list = json(&res.body);
    assert_eq!(list[0]["username"], "jones");
    assert_eq!(list[0]["boxes"].as_array().map(Vec::len), Some(2));
    let res = admin("GET", "/boxes?q=jones".into(), "").await;
    assert_eq!(
        json(&res.body).as_array().map(Vec::len),
        Some(2),
        "found by account"
    );
    let res = admin("GET", "/overview".into(), "").await;
    assert_eq!(json(&res.body)["accounts"]["all"], 2);
}
