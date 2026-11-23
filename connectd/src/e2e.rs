//! Pairing and the link over real iroh, on this machine, with no ZOOMRIYA
//! server and no relay: a pretend phone pairs at home, says hello, reaches a
//! web server with its label, and is cut off when removed.

use std::{sync::Arc, time::Duration};

use data_encoding::BASE64URL_NOPAD;
use iroh::{Endpoint, EndpointAddr, RelayMode, endpoint::presets};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use zoomriya_keys::{PublicKey, SecretKey, link_key_message};
use zoomriya_proto::link::{
    FromBox, LINK_ALPN, PAIR_ALPN, PairRequest, PairResponse, STREAM_CONTROL, STREAM_WEB, ToBox,
};

use crate::{
    config::Target,
    devices::Devices,
    link::{Link, LinkConfig, PROXY_SIGNATURE, TLV_DEVICE_NAME},
};

struct Phone {
    chip: SigningKey,
}

impl Phone {
    fn sig(&self, link: &PublicKey) -> String {
        let s: Signature = self.chip.sign(&link_key_message(link));
        BASE64URL_NOPAD.encode(s.to_der().as_bytes())
    }
}

async fn endpoint(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .relay_mode(RelayMode::Disabled)
        .bind_addr("127.0.0.1:0")
        .expect("addr")
        .bind()
        .await
        .expect("bind")
}

/// A pretend web server: answers with the PROXY header's device name and
/// what it was sent.
async fn web_server() -> std::net::SocketAddr {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = l.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut head = [0u8; 16];
                if s.read_exact(&mut head).await.is_err() {
                    return;
                }
                assert_eq!(&head[..12], &PROXY_SIGNATURE);
                let len = u16::from_be_bytes([head[14], head[15]]) as usize;
                let mut tlvs = vec![0u8; len];
                s.read_exact(&mut tlvs).await.expect("tlvs");
                let mut name = String::new();
                let mut i = 0;
                while i + 3 <= tlvs.len() {
                    let (t, l) = (
                        tlvs[i],
                        u16::from_be_bytes([tlvs[i + 1], tlvs[i + 2]]) as usize,
                    );
                    if t == TLV_DEVICE_NAME {
                        name = String::from_utf8_lossy(&tlvs[i + 3..i + 3 + l]).into();
                    }
                    i += 3 + l;
                }
                let mut body = [0u8; 5];
                s.read_exact(&mut body).await.expect("body");
                let reply = format!("{name}|{}", String::from_utf8_lossy(&body));
                s.write_all(reply.as_bytes()).await.expect("reply");
                s.shutdown().await.ok();
            });
        }
    });
    addr
}

async fn line<R: tokio::io::AsyncRead + Unpin>(r: &mut BufReader<R>) -> String {
    let mut s = String::new();
    tokio::time::timeout(Duration::from_secs(5), r.read_line(&mut s))
        .await
        .expect("in time")
        .expect("read");
    s
}

#[tokio::test]
async fn a_phone_from_pairing_to_removal() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().expect("dir");
    let devices = Arc::new(Devices::open(&dir.path().join("devices.json")).expect("devices"));
    let box_key = SecretKey::generate();
    let web = web_server().await;
    let record = zoomriya_keys::record::RecordSecret::generate().expect("secret");
    let secret_b64 = record.to_b64();
    let link = Link::start(
        box_key.clone(),
        LinkConfig {
            bind: vec!["127.0.0.1:0".parse().expect("addr")],
            targets: vec![Target {
                name: "gateway".into(),
                unix: None,
                tcp: Some(web),
                proxy_protocol: true,
            }],
            pairing_from_loopback: true,
            relay: None,
            onion: None,
            record: Some(record.clone()),
        },
        devices.clone(),
    )
    .await
    .expect("link");
    let box_addr: EndpointAddr = link.router.endpoint().addr();

    // A stranger knocks: turned away before anything.
    let stranger = endpoint(&SecretKey::generate()).await;
    let conn = stranger
        .connect(box_addr.clone(), LINK_ALPN)
        .await
        .expect("connects");
    let closed = tokio::time::timeout(Duration::from_secs(5), conn.closed())
        .await
        .expect("closed in time");
    assert!(format!("{closed:?}").contains("not paired"), "{closed:?}");

    // Pairing at home.
    let phone = Phone {
        chip: zoomriya_keys::new_test_chip(),
    };
    let link_key = SecretKey::generate();
    let ep = endpoint(&link_key).await;
    let secret = devices
        .start_pairing("Mum's phone", false, crate::now())
        .expect("code");
    let conn = ep
        .connect(box_addr.clone(), PAIR_ALPN)
        .await
        .expect("pairing connects");
    let (mut send, recv) = conn.open_bi().await.expect("stream");
    let req = PairRequest {
        secret,
        device_key: BASE64URL_NOPAD
            .encode(phone.chip.verifying_key().to_sec1_point(true).as_bytes()),
        link: link_key.public().into(),
        link_sig: phone.sig(&link_key.public()),
        name: "iPhone".into(),
    };
    let mut body = serde_json::to_vec(&req).expect("json");
    body.push(b'\n');
    send.write_all(&body).await.expect("send");
    send.finish().expect("finish");
    let mut recv = BufReader::new(recv);
    let answer: PairResponse = serde_json::from_str(&line(&mut recv).await).expect("answer");
    let PairResponse::Paired { addrs, record, .. } = answer else {
        panic!("{answer:?}")
    };
    assert!(!addrs.is_empty(), "where to reach the box directly");
    assert_eq!(record, Some(secret_b64.clone()), "how to find it from away");
    assert!(devices.by_link(&link_key.public()).is_some());
    conn.close(0u32.into(), b"done");

    // The family link: hello, then a web stream with its label.
    let conn = ep
        .connect(box_addr.clone(), LINK_ALPN)
        .await
        .expect("link connects");
    let (mut ctl_send, ctl_recv) = conn.open_bi().await.expect("control");
    ctl_send.write_all(&[STREAM_CONTROL]).await.expect("kind");
    let mut hello = serde_json::to_vec(&ToBox::Hello { version: 1 }).expect("json");
    hello.push(b'\n');
    ctl_send.write_all(&hello).await.expect("hello");
    let mut ctl_recv = BufReader::new(ctl_recv);
    let welcome: FromBox = serde_json::from_str(&line(&mut ctl_recv).await).expect("welcome");
    assert!(matches!(welcome, FromBox::Welcome { .. }), "{welcome:?}");

    let (mut send, mut recv) = conn.open_bi().await.expect("web");
    send.write_all(&[STREAM_WEB, 7]).await.expect("kind");
    send.write_all(b"gateway").await.expect("target");
    send.write_all(b"hello").await.expect("body");
    send.finish().expect("finish");
    let mut reply = String::new();
    tokio::time::timeout(Duration::from_secs(5), recv.read_to_string(&mut reply))
        .await
        .expect("in time")
        .expect("reply");
    assert_eq!(reply, "Mum's phone|hello");

    // An admin removes the phone: cut off at once, and turned away after.
    let id = devices.by_link(&link_key.public()).expect("listed").id;
    let removed = devices.remove(&id).expect("io").expect("removed");
    assert_eq!(link.live.cut(&removed.link.0), 1);
    tokio::time::timeout(Duration::from_secs(5), conn.closed())
        .await
        .expect("cut off at once");
    let conn = ep.connect(box_addr, LINK_ALPN).await.expect("connects");
    let closed = tokio::time::timeout(Duration::from_secs(5), conn.closed())
        .await
        .expect("refused");
    assert!(format!("{closed:?}").contains("not paired"));
}

#[tokio::test]
async fn a_laptop_pairs_with_the_short_code() {
    use zoomriya_link::{
        client::Client,
        device::SoftKey,
        pair::{self, PairError},
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().expect("dir");
    let devices = Arc::new(Devices::open(&dir.path().join("devices.json")).expect("devices"));
    let box_key = SecretKey::generate();
    let web = web_server().await;
    let link = Link::start(
        box_key.clone(),
        LinkConfig {
            bind: vec!["127.0.0.1:0".parse().expect("addr")],
            targets: vec![Target {
                name: "desktop".into(),
                unix: None,
                tcp: Some(web),
                proxy_protocol: true,
            }],
            pairing_from_loopback: true,
            relay: None,
            onion: None,
            record: None,
        },
        devices.clone(),
    )
    .await
    .expect("link");
    let addrs: Vec<std::net::SocketAddr> =
        link.router.endpoint().addr().ip_addrs().copied().collect();
    let device = SoftKey::generate().expect("key");
    let ep = zoomriya_link::endpoint(SecretKey::generate())
        .await
        .expect("endpoint");

    let code = devices
        .start_laptop_pairing("Anna's laptop", crate::now())
        .expect("code");
    // A wrong code: the box's confirmation fails, nothing is pinned, and a
    // try is gone.
    let wrong = pair::laptop(
        &ep,
        box_key.public(),
        &addrs,
        "ZZZZ-ZZZZ",
        &device,
        "MacBook",
    )
    .await;
    assert!(matches!(wrong, Err(PairError::WrongCode)), "{wrong:?}");
    assert!(devices.all().is_empty());

    // The code as typed, in small letters: paired, and the box pinned.
    let paired = pair::laptop(
        &ep,
        box_key.public(),
        &addrs,
        &code.to_lowercase(),
        &device,
        "MacBook",
    )
    .await
    .expect("paired");
    assert_eq!(paired.box_key, box_key.public());
    let listed = devices.by_link(&ep.id()).expect("listed");
    assert!(listed.laptop);
    assert_eq!(listed.name, "Anna's laptop");
    // The code is used up.
    let again = pair::laptop(&ep, box_key.public(), &addrs, &code, &device, "MacBook").await;
    assert!(again.is_err());

    // Now the laptop's link: a web stream to Box Desktop, labelled.
    let client = Client::new(
        ep,
        box_key.public(),
        paired.addrs,
        None,
        None,
        Arc::new(|_| {}),
    );
    let (mut send, mut recv) = client.web("desktop").await.expect("stream");
    send.write_all(b"hello").await.expect("body");
    send.finish().expect("finish");
    let mut reply = String::new();
    tokio::time::timeout(Duration::from_secs(5), recv.read_to_string(&mut reply))
        .await
        .expect("in time")
        .expect("reply");
    assert_eq!(reply, "Anna's laptop|hello");
    assert!(client.connected().await);
}
