//! What the relay and the control plane share: an HTTPS listener whose
//! certificate comes from files (reloaded every hour) or straight from Let's
//! Encrypt over TLS-ALPN, so nothing else needs installing on the server;
//! serving an axum router on it; the time; logging.

pub mod admin;
pub mod host;

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::Router;
use futures_util::StreamExt;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::{
    ServerConfig,
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use serde::Deserialize;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls_acme::{AcmeAcceptor, AcmeConfig, caches::DirCache};
use tower::ServiceExt;
use tracing::{debug, info, warn};

/// How long a connection may take to finish its TLS handshake.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Seconds since 1970, UTC.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Logs to standard error, filtered by `RUST_LOG` (default `info`). systemd's
/// journal picks it up.
pub fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

/// Where the HTTPS certificate comes from, as written in a server's config.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CertSource {
    /// PEM files, renewed by something else and read again every hour.
    Files {
        /// The certificate chain.
        cert: PathBuf,
        /// The private key.
        key: PathBuf,
    },
    /// Let's Encrypt, over TLS-ALPN on port 443. Nothing else is needed.
    LetsEncrypt {
        /// The names on the certificate. They must point at this server.
        domains: Vec<String>,
        /// Contact addresses, such as `mailto:ops@example.com`.
        #[serde(default)]
        contact: Vec<String>,
        /// Where the certificate and account key are kept between restarts.
        cache_dir: PathBuf,
        /// `false` uses Let's Encrypt's staging service, for testing.
        #[serde(default = "yes")]
        production: bool,
    },
}

fn yes() -> bool {
    true
}

/// A TCP listener that hands out TLS streams.
pub struct TlsListener {
    listener: TcpListener,
    config: Arc<ServerConfig>,
    acme: Option<AcmeAcceptor>,
}

impl TlsListener {
    /// Binds `addr` and sets up the certificate. With Let's Encrypt, a task
    /// keeps the certificate renewed for as long as the program runs.
    pub async fn bind(addr: SocketAddr, cert: &CertSource, alpn: &[&[u8]]) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .context("TLS versions")?
            .with_no_client_auth();
        let (mut config, acme) = match cert {
            CertSource::Files { cert, key } => {
                let resolver = FileResolver::load(provider, cert.clone(), key.clone())?;
                (builder.with_cert_resolver(resolver), None)
            }
            CertSource::LetsEncrypt {
                domains,
                contact,
                cache_dir,
                production,
            } => {
                let mut state = AcmeConfig::new(domains)
                    .contact(contact)
                    .cache(DirCache::new(cache_dir.clone()))
                    .directory_lets_encrypt(*production)
                    .state();
                let resolver = state.resolver();
                let acceptor = state.acceptor();
                tokio::spawn(async move {
                    while let Some(event) = state.next().await {
                        match event {
                            Ok(ok) => info!("certificate: {ok:?}"),
                            Err(err) => warn!("certificate: {err:?}"),
                        }
                    }
                });
                (builder.with_cert_resolver(resolver), Some(acceptor))
            }
        };
        config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("listening on {addr}"))?;
        info!("HTTPS on {}", listener.local_addr()?);
        Ok(Self {
            listener,
            config: Arc::new(config),
            acme,
        })
    }

    /// The address it's bound to.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Waits for the next TCP connection.
    pub async fn accept(&self) -> std::io::Result<(TcpStream, SocketAddr)> {
        self.listener.accept().await
    }

    /// The TLS settings, for passing to code that does its own handshake.
    pub fn config(&self) -> Arc<ServerConfig> {
        self.config.clone()
    }

    /// Does the TLS handshake. `None` when the connection was Let's Encrypt
    /// checking us, which needs nothing more.
    pub async fn handshake(&self, tcp: TcpStream) -> std::io::Result<Option<TlsStream<TcpStream>>> {
        let fut = async {
            match &self.acme {
                None => tokio_rustls::TlsAcceptor::from(self.config.clone())
                    .accept(tcp)
                    .await
                    .map(Some),
                Some(acme) => match acme.accept(tcp).await? {
                    None => Ok(None),
                    Some(start) => start.into_stream(self.config.clone()).await.map(Some),
                },
            }
        };
        tokio::time::timeout(HANDSHAKE_TIMEOUT, fut)
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TLS handshake"))?
    }
}

/// Serves `router` over HTTP/1.1 on an established stream. The router sees
/// the peer's address as `axum::extract::ConnectInfo<SocketAddr>`.
pub async fn serve_router<S>(stream: S, peer: SocketAddr, router: Router)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service =
        hyper::service::service_fn(move |mut req: hyper::Request<hyper::body::Incoming>| {
            req.extensions_mut()
                .insert(axum::extract::ConnectInfo(peer));
            router.clone().oneshot(req.map(axum::body::Body::new))
        });
    let conn = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
        .http1_only()
        .serve_connection_with_upgrades(TokioIo::new(stream), service)
        .into_owned();
    if let Err(err) = conn.await {
        debug!("connection from {peer}: {err}");
    }
}

/// Serves `router` on every connection the listener accepts, forever.
pub async fn serve_https(listener: TlsListener, router: Router) -> Result<()> {
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
        let listener = listener.clone();
        let router = router.clone();
        tokio::spawn(async move {
            match listener.handshake(tcp).await {
                Ok(Some(tls)) => serve_router(tls, peer, router).await,
                Ok(None) => {}
                Err(err) => debug!("TLS from {peer}: {err}"),
            }
        });
    }
}

/// Serves `router` without TLS: for addresses only reachable from the server
/// itself, and for tests.
pub async fn serve_plain(listener: TcpListener, router: Router) -> Result<()> {
    loop {
        let (tcp, peer) = listener.accept().await?;
        let router = router.clone();
        tokio::spawn(serve_router(tcp, peer, router));
    }
}

/// Reads a certificate and key from PEM files, and again every hour, so a
/// renewal on disk is picked up without a restart.
#[derive(Debug)]
struct FileResolver {
    current: RwLock<Arc<CertifiedKey>>,
}

impl FileResolver {
    fn load(provider: Arc<CryptoProvider>, cert: PathBuf, key: PathBuf) -> Result<Arc<Self>> {
        let first = read_cert(&provider, &cert, &key)?;
        let this = Arc::new(Self {
            current: RwLock::new(Arc::new(first)),
        });
        let weak = Arc::downgrade(&this);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            tick.tick().await;
            loop {
                tick.tick().await;
                let Some(this) = weak.upgrade() else { return };
                match read_cert(&provider, &cert, &key) {
                    Ok(ck) => {
                        if let Ok(mut cur) = this.current.write() {
                            *cur = Arc::new(ck);
                        }
                    }
                    Err(err) => warn!("reading certificate again: {err:#}"),
                }
            }
        });
        Ok(this)
    }
}

impl ResolvesServerCert for FileResolver {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current.read().ok().map(|c| c.clone())
    }
}

fn read_cert(provider: &CryptoProvider, cert: &PathBuf, key: &PathBuf) -> Result<CertifiedKey> {
    let chain = CertificateDer::pem_file_iter(cert)
        .with_context(|| format!("reading {}", cert.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("reading {}", cert.display()))?;
    anyhow::ensure!(!chain.is_empty(), "no certificate in {}", cert.display());
    let key =
        PrivateKeyDer::from_pem_file(key).with_context(|| format!("reading {}", key.display()))?;
    CertifiedKey::from_der(chain, key, provider).context("certificate and key don't match")
}

/// Waits for Ctrl-C, or SIGTERM from systemd.
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
