//! The box in the laptop's own browser: `http://zoomriya.localhost:7480`
//! for the family apps, `http://zoomriyabox.localhost:7480` for Box
//! Desktop (docs/direct-plus.md, §10). Browsers keep `*.localhost` on the
//! laptop and treat it as secure, so no certificate is needed. Each request
//! goes to the box on its own iroh stream, which is encrypted and pinned to
//! the box key.
//!
//! | Check | Keeps out |
//! |---|---|
//! | Listens on `127.0.0.1` and `[::1]` only | Other computers |
//! | Only our two host names, with the port | Web pages using DNS rebinding |
//! | The session cookie, set only through a launch link from the app | Other programs and users on the laptop |
//!
//! Box Desktop gets its cookie through the apps' address
//! (`/.zoomriya/hop`), so opening ZOOMRIYA once is enough for both. The two
//! names are separate sites to the browser, so the cookie is `SameSite=Lax`:
//! the hop is a top-level move from one to the other. It only proves the
//! browser was opened by the app; the box's own sign-in and Origin checks
//! still guard every change.

use std::{
    collections::HashMap,
    convert::Infallible,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Result;
use bytes::Bytes;
use data_encoding::BASE64URL_NOPAD;
use http::{HeaderValue, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
use subtle::ConstantTimeEq;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, warn};

/// The port, on the laptop only.
pub const PORT: u16 = 7480;
/// The family apps' host name.
pub const APPS_HOST: &str = "zoomriya.localhost";
/// Box Desktop's host name.
pub const DESKTOP_HOST: &str = "zoomriyabox.localhost";
const COOKIE: &str = "zoomriya_laptop";
/// How long a launch link works.
const LAUNCH_LIFETIME: Duration = Duration::from_secs(120);

/// A stream to one of the box's web servers.
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Io for T {}

/// The future [`Upstream::open`] returns.
pub type Opening = Pin<Box<dyn Future<Output = Result<Box<dyn Io>>> + Send>>;

/// Where requests go: the box, over iroh.
pub trait Upstream: Send + Sync + 'static {
    /// A fresh stream to the web server called `target` on the box.
    fn open(&self, target: &'static str) -> Opening;
}

/// Which of the two addresses a request came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site {
    /// The family apps, through the box's gateway.
    Apps,
    /// Box Desktop.
    Desktop,
}

impl Site {
    fn host(self) -> &'static str {
        match self {
            Site::Apps => APPS_HOST,
            Site::Desktop => DESKTOP_HOST,
        }
    }

    /// The box's web server for this site, as named in connectd's settings.
    pub fn target(self) -> &'static str {
        match self {
            Site::Apps => "apps",
            Site::Desktop => "desktop",
        }
    }
}

type Body = BoxBody<Bytes, hyper::Error>;

fn full(s: impl Into<Bytes>) -> Body {
    Full::new(s.into())
        .map_err(|n: Infallible| match n {})
        .boxed()
}

fn page(status: StatusCode, title: &str, words: &str) -> Response<Body> {
    let html = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <title>{title}</title><style>body{{font:16px system-ui,sans-serif;max-width:32rem;\
         margin:15vh auto;padding:0 1rem;color:#15232a}}@media(prefers-color-scheme:dark)\
         {{body{{background:#162026;color:#e2eaed}}}}</style><h1>{title}</h1><p>{words}</p>"
    );
    let mut r = Response::new(full(html));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn redirect(to: &str, cookie: Option<String>) -> Response<Body> {
    let mut r = Response::new(full(""));
    *r.status_mut() = StatusCode::SEE_OTHER;
    if let Ok(v) = HeaderValue::from_str(to) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    if let Some(c) = cookie.and_then(|c| HeaderValue::from_str(&c).ok()) {
        r.headers_mut().insert(header::SET_COOKIE, c);
    }
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn query(req: &Request<Incoming>, key: &str) -> Option<String> {
    req.uri().query()?.split('&').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k == key).then(|| decode(v)).flatten()
    })
}

/// Only a path on the same site: never `//elsewhere` or a full address.
fn safe_next(next: Option<String>) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n,
        _ => "/".into(),
    }
}

/// The browser server.
pub struct Server<U> {
    upstream: U,
    secret: String,
    port: u16,
    launches: Mutex<HashMap<String, (Site, Instant)>>,
}

impl<U> std::fmt::Debug for Server<U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").field("port", &self.port).finish()
    }
}

impl<U: Upstream> Server<U> {
    /// A server sending requests to `upstream`; `secret` is the session
    /// cookie's value.
    pub fn new(upstream: U, secret: String, port: u16) -> Arc<Self> {
        Arc::new(Self {
            upstream,
            secret,
            port,
            launches: Mutex::default(),
        })
    }

    fn origin(&self, site: Site) -> String {
        format!("http://{}:{}", site.host(), self.port)
    }

    /// A one-time link that signs the browser in to `site` and opens `next`.
    /// This is what **Open ZOOMRIYA** opens.
    pub fn launch_url(&self, site: Site, next: &str) -> Result<String> {
        let mut b = [0u8; 24];
        getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
        let t = BASE64URL_NOPAD.encode(&b);
        if let Ok(mut l) = self.launches.lock() {
            let now = Instant::now();
            l.retain(|_, (_, at)| now.duration_since(*at) < LAUNCH_LIFETIME);
            l.insert(t.clone(), (site, now));
        }
        Ok(format!(
            "{}/.zoomriya/open?t={t}&next={}",
            self.origin(site),
            encode(&safe_next(Some(next.into())))
        ))
    }

    fn take_launch(&self, t: &str, site: Site) -> bool {
        self.launches
            .lock()
            .ok()
            .and_then(|mut l| l.remove(t))
            .is_some_and(|(s, at)| s == site && at.elapsed() < LAUNCH_LIFETIME)
    }

    fn site(&self, req: &Request<Incoming>) -> Option<Site> {
        let host = req.headers().get(header::HOST)?.to_str().ok()?;
        let (name, port) = host.rsplit_once(':')?;
        if port.parse::<u16>().ok()? != self.port {
            return None;
        }
        [Site::Apps, Site::Desktop]
            .into_iter()
            .find(|s| name.eq_ignore_ascii_case(s.host()))
    }

    fn signed_in(&self, req: &Request<Incoming>) -> bool {
        req.headers()
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|c| c.trim().split_once('='))
            .any(|(k, v)| k == COOKIE && bool::from(v.as_bytes().ct_eq(self.secret.as_bytes())))
    }

    /// Takes our own cookie out before the request goes to the box.
    fn strip_cookie(req: &mut Request<Incoming>) {
        let kept: Vec<String> = req
            .headers()
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .map(str::trim)
            .filter(|c| !c.is_empty() && !c.starts_with(&format!("{COOKIE}=")))
            .map(String::from)
            .collect();
        req.headers_mut().remove(header::COOKIE);
        if !kept.is_empty()
            && let Ok(v) = HeaderValue::from_str(&kept.join("; "))
        {
            req.headers_mut().insert(header::COOKIE, v);
        }
    }

    /// Answers one request.
    pub async fn handle(self: Arc<Self>, mut req: Request<Incoming>) -> Response<Body> {
        let Some(site) = self.site(&req) else {
            return page(
                StatusCode::MISDIRECTED_REQUEST,
                "Not here",
                "This address is not one of ZOOMRIYA's.",
            );
        };
        match req.uri().path() {
            "/.zoomriya/open" => {
                let ok = query(&req, "t").is_some_and(|t| self.take_launch(&t, site));
                if !ok {
                    return page(
                        StatusCode::FORBIDDEN,
                        "Open ZOOMRIYA from the app",
                        "That link has already been used or is too old. Choose Open ZOOMRIYA in the ZOOMRIYA app on this laptop.",
                    );
                }
                let cookie = format!("{COOKIE}={}; HttpOnly; SameSite=Lax; Path=/", self.secret);
                return redirect(&safe_next(query(&req, "next")), Some(cookie));
            }
            "/.zoomriya/hop" if site == Site::Apps && self.signed_in(&req) => {
                return match self.launch_url(Site::Desktop, &safe_next(query(&req, "next"))) {
                    Ok(url) => redirect(&url, None),
                    Err(_) => page(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Try again",
                        "Something went wrong in the ZOOMRIYA app.",
                    ),
                };
            }
            _ => {}
        }
        if !self.signed_in(&req) {
            return match site {
                Site::Desktop => {
                    let next = req
                        .uri()
                        .path_and_query()
                        .map(|p| p.as_str())
                        .unwrap_or("/");
                    redirect(
                        &format!(
                            "{}/.zoomriya/hop?next={}",
                            self.origin(Site::Apps),
                            encode(next)
                        ),
                        None,
                    )
                }
                Site::Apps => page(
                    StatusCode::FORBIDDEN,
                    "Open ZOOMRIYA from the app",
                    "Choose Open ZOOMRIYA in the ZOOMRIYA app on this laptop.",
                ),
            };
        }
        Self::strip_cookie(&mut req);
        match self.forward(site, req).await {
            Ok(r) => r,
            Err(e) => {
                debug!("forwarding: {e:#}");
                page(
                    StatusCode::BAD_GATEWAY,
                    "Your box cannot be reached",
                    "The ZOOMRIYA app keeps trying. Check that this laptop is online, and that the box is on.",
                )
            }
        }
    }

    async fn forward(&self, site: Site, mut req: Request<Incoming>) -> Result<Response<Body>> {
        let io = self.upstream.open(site.target()).await?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(io)).await?;
        tokio::spawn(async move {
            if let Err(e) = conn.with_upgrades().await {
                debug!("to the box: {e}");
            }
        });
        let wants_upgrade = req.headers().contains_key(header::UPGRADE);
        let from_browser = wants_upgrade.then(|| hyper::upgrade::on(&mut req));
        let mut resp = sender.send_request(req).await?;
        if resp.status() == StatusCode::SWITCHING_PROTOCOLS
            && let Some(from_browser) = from_browser
        {
            let to_box = hyper::upgrade::on(&mut resp);
            tokio::spawn(async move {
                match tokio::try_join!(from_browser, to_box) {
                    Ok((a, b)) => {
                        let _ = tokio::io::copy_bidirectional(
                            &mut TokioIo::new(a),
                            &mut TokioIo::new(b),
                        )
                        .await;
                    }
                    Err(e) => debug!("upgrade: {e}"),
                }
            });
        }
        Ok(resp.map(|b| b.boxed()))
    }

    /// Takes the port on the laptop's loopback addresses. Fails when it is
    /// taken: most likely the app is already running.
    pub async fn bind(&self) -> Result<Vec<tokio::net::TcpListener>> {
        let mut listeners = Vec::new();
        for ip in [
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            std::net::IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            match tokio::net::TcpListener::bind(SocketAddr::new(ip, self.port)).await {
                Ok(l) => listeners.push(l),
                Err(e) => warn!("listening on {ip}: {e}"),
            }
        }
        anyhow::ensure!(
            !listeners.is_empty(),
            "port {} is taken: is the ZOOMRIYA app already running?",
            self.port
        );
        Ok(listeners)
    }

    /// Serves on `listeners` until the task is dropped.
    pub async fn serve(self: Arc<Self>, listeners: Vec<tokio::net::TcpListener>) {
        let mut tasks = tokio::task::JoinSet::new();
        for l in listeners {
            let me = self.clone();
            tasks.spawn(async move { me.accept(l).await });
        }
        while tasks.join_next().await.is_some() {}
    }

    /// Takes the port and serves until the task is dropped.
    pub async fn run(self: Arc<Self>) -> Result<()> {
        let listeners = self.bind().await?;
        self.serve(listeners).await;
        Ok(())
    }

    /// Serves one listener.
    pub async fn accept(self: Arc<Self>, listener: tokio::net::TcpListener) {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                continue;
            };
            let me = self.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    let me = me.clone();
                    async move { Ok::<_, Infallible>(me.handle(req).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tcp), service)
                    .with_upgrades()
                    .await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A pretend box: records each request's head, and answers. A request
    /// asking to upgrade gets 101 and an echo.
    #[derive(Clone)]
    struct FakeBox {
        addr: SocketAddr,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl FakeBox {
        async fn start() -> Self {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = l.local_addr().expect("addr");
            let seen: Arc<Mutex<Vec<String>>> = Arc::default();
            let s = seen.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut t, _)) = l.accept().await else {
                        return;
                    };
                    let s = s.clone();
                    tokio::spawn(async move {
                        let mut head = Vec::new();
                        let mut b = [0u8; 1];
                        while !head.ends_with(b"\r\n\r\n") {
                            if t.read_exact(&mut b).await.is_err() {
                                return;
                            }
                            head.push(b[0]);
                        }
                        let head = String::from_utf8_lossy(&head).to_string();
                        let upgrade = head.to_lowercase().contains("upgrade: echo");
                        s.lock().expect("lock").push(head);
                        if upgrade {
                            let _ = t
                                .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: echo\r\nConnection: Upgrade\r\n\r\n")
                                .await;
                            let (mut r, mut w) = t.split();
                            let _ = tokio::io::copy(&mut r, &mut w).await;
                        } else {
                            let _ = t
                                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nSet-Cookie: app=1\r\n\r\nfrom-b")
                                .await;
                        }
                    });
                }
            });
            Self { addr, seen }
        }
    }

    impl Upstream for FakeBox {
        fn open(&self, target: &'static str) -> Opening {
            let addr = self.addr;
            let seen = self.seen.clone();
            Box::pin(async move {
                seen.lock().expect("lock").push(format!("target {target}"));
                Ok(Box::new(tokio::net::TcpStream::connect(addr).await?) as Box<dyn Io>)
            })
        }
    }

    async fn laptop(fake: &FakeBox) -> (Arc<Server<FakeBox>>, SocketAddr) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = l.local_addr().expect("addr");
        let server = Server::new(fake.clone(), "s3cret".into(), addr.port());
        tokio::spawn(server.clone().accept(l));
        (server, addr)
    }

    /// Sends a request with `Connection: close`, returns the whole answer.
    async fn ask(to: SocketAddr, host: &str, path: &str, extra: &str) -> String {
        let mut t = tokio::net::TcpStream::connect(to).await.expect("connect");
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{}\r\n{extra}Connection: close\r\n\r\n",
            to.port()
        );
        t.write_all(req.as_bytes()).await.expect("send");
        let mut out = String::new();
        tokio::time::timeout(Duration::from_secs(5), t.read_to_string(&mut out))
            .await
            .expect("in time")
            .expect("read");
        out
    }

    fn header<'a>(resp: &'a str, name: &str) -> Option<&'a str> {
        resp.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    #[tokio::test]
    async fn only_our_names_and_the_cookie_get_through() {
        let fake = FakeBox::start().await;
        let (_, at) = laptop(&fake).await;
        // Another name, as with DNS rebinding.
        let r = ask(at, "evil.example", "/", "").await;
        assert!(r.starts_with("HTTP/1.1 421"), "{r}");
        // No cookie: the apps say to use the app, Box Desktop hops to them.
        let r = ask(at, APPS_HOST, "/photos", "").await;
        assert!(r.starts_with("HTTP/1.1 403"), "{r}");
        let r = ask(at, DESKTOP_HOST, "/people?x=1", "").await;
        assert!(r.starts_with("HTTP/1.1 303"), "{r}");
        assert_eq!(
            header(&r, "location"),
            Some(
                format!(
                    "http://{APPS_HOST}:{}/.zoomriya/hop?next=/people%3Fx%3D1",
                    at.port()
                )
                .as_str()
            )
        );
        // A wrong cookie.
        let r = ask(at, APPS_HOST, "/", "Cookie: zoomriya_laptop=guess\r\n").await;
        assert!(r.starts_with("HTTP/1.1 403"), "{r}");
        assert!(
            fake.seen.lock().expect("lock").is_empty(),
            "nothing reached the box"
        );
    }

    #[tokio::test]
    async fn launch_links_sign_the_browser_in_once() {
        let fake = FakeBox::start().await;
        let (server, at) = laptop(&fake).await;
        let url = server.launch_url(Site::Apps, "/photos").expect("url");
        let path = url.split_once(&format!(":{}", at.port())).expect("port").1;
        let r = ask(at, APPS_HOST, path, "").await;
        assert!(r.starts_with("HTTP/1.1 303"), "{r}");
        assert_eq!(header(&r, "location"), Some("/photos"));
        let cookie = header(&r, "set-cookie").expect("cookie");
        assert!(cookie.starts_with("zoomriya_laptop=s3cret;"), "{cookie}");
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
        // Used up.
        let r = ask(at, APPS_HOST, path, "").await;
        assert!(r.starts_with("HTTP/1.1 403"), "{r}");
        // Not for the other site, and never to another place.
        let url = server
            .launch_url(Site::Apps, "//evil.example/")
            .expect("url");
        assert!(url.ends_with("next=/"), "{url}");
        let path = url.split_once(&format!(":{}", at.port())).expect("port").1;
        let r = ask(at, DESKTOP_HOST, path, "").await;
        assert!(!r.contains("set-cookie: zoomriya_laptop"), "{r}");
    }

    #[tokio::test]
    async fn signed_in_requests_reach_the_box_without_our_cookie() {
        let fake = FakeBox::start().await;
        let (_, at) = laptop(&fake).await;
        let r = ask(
            at,
            DESKTOP_HOST,
            "/api/people",
            "Cookie: theirs=1; zoomriya_laptop=s3cret; other=2\r\n",
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        assert!(r.ends_with("from-b"), "{r}");
        assert_eq!(header(&r, "set-cookie"), Some("app=1"));
        let seen = fake.seen.lock().expect("lock").clone();
        assert_eq!(seen[0], "target desktop");
        let head = &seen[1];
        assert!(head.starts_with("GET /api/people HTTP/1.1\r\n"), "{head}");
        assert!(head.contains("cookie: theirs=1; other=2\r\n"), "{head}");
        assert!(!head.contains("s3cret"), "{head}");
    }

    #[tokio::test]
    async fn box_desktop_gets_its_cookie_through_the_apps() {
        let fake = FakeBox::start().await;
        let (_, at) = laptop(&fake).await;
        let r = ask(
            at,
            APPS_HOST,
            "/.zoomriya/hop?next=/people",
            "Cookie: zoomriya_laptop=s3cret\r\n",
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 303"), "{r}");
        let to = header(&r, "location").expect("location");
        let prefix = format!("http://{DESKTOP_HOST}:{}", at.port());
        let path = to.strip_prefix(&prefix).expect("to Box Desktop");
        let r = ask(at, DESKTOP_HOST, path, "").await;
        assert_eq!(header(&r, "location"), Some("/people"));
        assert!(header(&r, "set-cookie").is_some_and(|c| c.starts_with("zoomriya_laptop=s3cret")));
        // Without the apps' cookie, no hop.
        let r = ask(at, APPS_HOST, "/.zoomriya/hop?next=/", "").await;
        assert!(r.starts_with("HTTP/1.1 403"), "{r}");
    }

    #[tokio::test]
    async fn upgrades_pass_through() {
        let fake = FakeBox::start().await;
        let (_, at) = laptop(&fake).await;
        let mut t = tokio::net::TcpStream::connect(at).await.expect("connect");
        let req = format!(
            "GET /ws/chat HTTP/1.1\r\nHost: {APPS_HOST}:{}\r\nCookie: zoomriya_laptop=s3cret\r\nConnection: Upgrade\r\nUpgrade: echo\r\n\r\n",
            at.port()
        );
        t.write_all(req.as_bytes()).await.expect("send");
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            t.read_exact(&mut b).await.expect("head");
            head.push(b[0]);
        }
        assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));
        t.write_all(b"ping").await.expect("ping");
        let mut back = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(5), t.read_exact(&mut back))
            .await
            .expect("in time")
            .expect("echo");
        assert_eq!(&back, b"ping");
    }
}
