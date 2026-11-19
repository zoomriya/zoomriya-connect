//! The outside check: does the box's door work, and does it lead only to the
//! album viewer? The control plane only ever checks the asking box's own
//! address, so it can't be used to scan anyone else.
//!
//! What the album viewer must do for the check to pass:
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /.well-known/zoomriya-door` | `200`, body starting `zoomriya-album-viewer` |
//! | Any path it doesn't serve, `/id/` included | `404` |

#![allow(clippy::result_large_err)]

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, Method, Uri},
    response::{IntoResponse, Response},
};
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_rustls::TlsConnector;
use zoomriya_proto::{DoorCheck, DoorVerdict, addrs, names};

use crate::api::{Api, by_dns_key, slow_down};

/// The album viewer's answer to the check path starts with this.
pub const VIEWER_MARK: &str = "zoomriya-album-viewer";
/// Paths that must never answer through the door: ZOOMRIYA ID's sign-in and
/// the apps' API.
const MUST_NOT_ANSWER: &[&str] = &["/id/", "/id/api/v1/auth/session", "/api/", "/"];

/// Checks doors.
#[derive(Clone)]
pub struct DoorChecker {
    port: u16,
    tls: Arc<ClientConfig>,
    timeout: Duration,
}

impl std::fmt::Debug for DoorChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DoorChecker(port {})", self.port)
    }
}

impl DoorChecker {
    /// Checks port 443, trusting the usual certificate authorities.
    pub fn new() -> Self {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self::with(443, roots)
    }

    /// Checks another port, trusting `roots`: for tests.
    pub fn with(port: u16, roots: RootCertStore) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map(|b| b.with_root_certificates(roots).with_no_client_auth())
            .unwrap_or_else(|_| {
                ClientConfig::builder()
                    .with_root_certificates(RootCertStore::empty())
                    .with_no_client_auth()
            });
        Self {
            port,
            tls: Arc::new(tls),
            timeout: Duration::from_secs(5),
        }
    }

    async fn get(&self, ip: IpAddr, host: &str, path: &str) -> Result<(u16, String), Step> {
        let addr = SocketAddr::new(ip, self.port);
        let tcp = tokio::time::timeout(self.timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| Step::Connect)?
            .map_err(|_| Step::Connect)?;
        let name = ServerName::try_from(host.to_string()).map_err(|_| Step::Tls)?;
        let mut tls = tokio::time::timeout(
            self.timeout,
            TlsConnector::from(self.tls.clone()).connect(name, tcp),
        )
        .await
        .map_err(|_| Step::Tls)?
        .map_err(|_| Step::Tls)?;
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: zoomriya-door-check\r\nConnection: close\r\n\r\n"
        );
        let io = async {
            tls.write_all(req.as_bytes()).await?;
            let mut buf = Vec::with_capacity(4096);
            let mut chunk = [0u8; 4096];
            while buf.len() < 16 * 1024 {
                let n = tls.read(&mut chunk).await?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Ok::<_, std::io::Error>(buf)
        };
        let buf = match tokio::time::timeout(self.timeout, io).await {
            Ok(Ok(b)) => b,
            // A server that closes without TLS's goodbye still answered.
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Vec::new(),
            _ => return Err(Step::Http),
        };
        let text = String::from_utf8_lossy(&buf);
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or(Step::Http)?;
        let body = text
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or_default();
        Ok((status, body))
    }

    /// Checks the door at `ip`, reached by the name `host`.
    pub async fn check(&self, ip: IpAddr, host: &str) -> DoorCheck {
        let address = ip.to_string();
        let verdict = |verdict, detail: &str| DoorCheck {
            address: address.clone(),
            verdict,
            detail: detail.into(),
        };
        match self.get(ip, host, "/.well-known/zoomriya-door").await {
            Err(Step::Connect) => {
                return verdict(
                    DoorVerdict::NoDoor,
                    "Nothing answered from the internet. Your router didn't open the door, so guests will connect another way.",
                );
            }
            Err(Step::Tls) => {
                return verdict(
                    DoorVerdict::WrongService,
                    "Something answered, but not with your box's certificate. Check your router's port forwarding.",
                );
            }
            Err(Step::Http) => {
                return verdict(
                    DoorVerdict::WrongService,
                    "Something answered, but not the album viewer.",
                );
            }
            Ok((200, body)) if body.starts_with(VIEWER_MARK) => {}
            Ok(_) => {
                return verdict(
                    DoorVerdict::WrongService,
                    "Something answered, but not the album viewer.",
                );
            }
        }
        for path in MUST_NOT_ANSWER {
            match self.get(ip, host, path).await {
                Ok((404, _)) | Err(Step::Connect) => {}
                _ => {
                    return verdict(
                        DoorVerdict::Leaks,
                        "The sign-in page or an app can be reached through the door. Your box closed it.",
                    );
                }
            }
        }
        verdict(
            DoorVerdict::Ok,
            "Guests can open share links directly, and reach only the album viewer.",
        )
    }
}

#[derive(Debug)]
enum Step {
    Connect,
    Tls,
    Http,
}

/// `POST /v1/boxes/{name}/check`: checks the door at the address the request
/// came from, or, over IPv6, the door address the box registered in the same /56.
pub async fn check(
    State(api): State<Arc<Api>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let name = match by_dns_key(&api, &name, &method, &uri, &headers, &body, peer.ip()) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let Ok(Some(row)) = api.store.find(&name) else {
        return slow_down();
    };
    if !api.per_box.allow(&row.box_key) {
        return slow_down();
    }
    let from = match peer.ip() {
        IpAddr::V6(a) => a.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(a)),
        v4 => v4,
    };
    let target = match from {
        IpAddr::V4(_) => from,
        IpAddr::V6(_) => api
            .store
            .entry(&name)
            .and_then(|e| e.door_v6)
            .map(IpAddr::V6)
            .filter(|d| addrs::same_home(*d, from))
            .unwrap_or(from),
    };
    let host = format!("{}.{}", names::share_name(&name), api.zone);
    let result = api.checker.check(target, &host).await;
    tracing::info!(%name, verdict = ?result.verdict, "door checked");
    Json(result).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    /// A pretend door: answers the check path with `mark`, and `/id/` with `id_status`.
    async fn door(mark: &'static str, id_status: u16) -> (u16, RootCertStore) {
        let cert = rcgen::generate_simple_self_signed(vec!["maple-share.zoomriya.app".into()])
            .expect("cert");
        let mut roots = RootCertStore::empty();
        roots.add(cert.cert.der().clone()).expect("root");
        let key = rustls::pki_types::PrivateKeyDer::try_from(cert.signing_key.serialize_der())
            .expect("key");
        let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key)
        .expect("server config");
        let acceptor = TlsAcceptor::from(Arc::new(cfg));
        let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = l.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = l.accept().await else {
                    return;
                };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(mut s) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut buf = [0u8; 2048];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req.split_whitespace().nth(1).unwrap_or("");
                    let (status, body) = if path == "/.well-known/zoomriya-door" {
                        (200, mark)
                    } else if path == "/id/" {
                        (id_status, "sign in")
                    } else {
                        (404, "")
                    };
                    let res = format!(
                        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(res.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        (port, roots)
    }

    const HOST: &str = "maple-share.zoomriya.app";
    fn local() -> IpAddr {
        IpAddr::from([127, 0, 0, 1])
    }

    #[tokio::test]
    async fn a_good_door() {
        let (port, roots) = door("zoomriya-album-viewer 1", 404).await;
        let r = DoorChecker::with(port, roots).check(local(), HOST).await;
        assert_eq!(r.verdict, DoorVerdict::Ok, "{}", r.detail);
    }

    #[tokio::test]
    async fn a_leaking_door() {
        let (port, roots) = door("zoomriya-album-viewer 1", 200).await;
        let r = DoorChecker::with(port, roots).check(local(), HOST).await;
        assert_eq!(r.verdict, DoorVerdict::Leaks);
    }

    #[tokio::test]
    async fn the_wrong_service() {
        let (port, roots) = door("some router page", 404).await;
        let r = DoorChecker::with(port, roots.clone())
            .check(local(), HOST)
            .await;
        assert_eq!(r.verdict, DoorVerdict::WrongService);
        // The right service, the wrong certificate.
        let r = DoorChecker::with(port, roots)
            .check(local(), "oak-share.zoomriya.app")
            .await;
        assert_eq!(r.verdict, DoorVerdict::WrongService);
    }

    #[tokio::test]
    async fn no_door() {
        let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = l.local_addr().expect("addr").port();
        drop(l);
        let r = DoorChecker::with(port, RootCertStore::empty())
            .check(local(), HOST)
            .await;
        assert_eq!(r.verdict, DoorVerdict::NoDoor);
    }
}
