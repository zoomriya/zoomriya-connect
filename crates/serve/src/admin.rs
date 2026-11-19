//! The admin side both servers share: the GUI's files and the API behind it.
//!
//! The GUI listens on its own address, `127.0.0.1:8444` unless set otherwise,
//! so it's off the internet: reach it through an SSH tunnel
//! (`ssh -L 8444:127.0.0.1:8444 server`). A family that wants it on the
//! network gives it a certificate.
//!
//! Signing in takes a token made with the server's `admin-token` command. The
//! server keeps only each token's SHA-256. The GUI sends the token with every
//! request (`Authorization: Bearer`), so there's no cookie a page elsewhere
//! could ride on.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use data_encoding::{BASE64URL_NOPAD, HEXLOWER};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tower_http::services::{ServeDir, ServeFile};

use crate::CertSource;

/// The admin settings, as written in either server's config.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConfig {
    /// Where the GUI listens. Keep it on `127.0.0.1` unless it has a certificate.
    #[serde(default = "local")]
    pub bind: SocketAddr,
    /// The tokens' hashes, one `name sha256` a line.
    pub tokens_file: PathBuf,
    /// The GUI's built files.
    #[serde(default = "web_dir")]
    pub web_dir: PathBuf,
    /// A certificate, to serve the GUI over HTTPS on the network.
    #[serde(default)]
    pub cert: Option<CertSource>,
}

fn local() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8444))
}

fn web_dir() -> PathBuf {
    PathBuf::from("/usr/share/zoomriya-connect/admin")
}

/// The signed-in admin's name, for the log.
#[derive(Debug, Clone)]
pub struct AdminName(pub String);

/// The admin tokens' hashes, read again on each request so a new or removed
/// token works at once.
#[derive(Debug, Clone)]
pub struct Tokens {
    file: Arc<PathBuf>,
}

impl Tokens {
    /// Tokens kept in `file`.
    pub fn new(file: PathBuf) -> Self {
        Self {
            file: Arc::new(file),
        }
    }

    /// The admin's name, if `token` is one of ours.
    pub fn check(&self, token: &str) -> Option<String> {
        let text = std::fs::read_to_string(&*self.file).ok()?;
        let hash = HEXLOWER.encode(&Sha256::digest(token.as_bytes()));
        let mut found = None;
        for line in text.lines() {
            let Some((name, h)) = line.trim().split_once(' ') else {
                continue;
            };
            // Compare every line, so the time taken doesn't say which one.
            if bool::from(h.trim().as_bytes().ct_eq(hash.as_bytes())) {
                found = Some(name.to_string());
            }
        }
        found
    }
}

/// Makes a token for `name`, keeps its hash in `file`, and returns the token
/// for showing once.
pub fn new_token(file: &Path, name: &str) -> Result<String> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 40
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "names are letters, numbers, - _ and ., up to 40"
    );
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
    let token = format!("zat_{}", BASE64URL_NOPAD.encode(&bytes));
    let hash = HEXLOWER.encode(&Sha256::digest(token.as_bytes()));
    let mut text = std::fs::read_to_string(file).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{name} {hash}\n"));
    if let Some(dir) = file.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    write_private(file, text.as_bytes())?;
    Ok(token)
}

/// Removes `name`'s tokens. Returns how many.
pub fn remove_tokens(file: &Path, name: &str) -> Result<usize> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let mut removed = 0;
    let kept: String = text
        .lines()
        .filter(|l| {
            let gone = l.split_once(' ').is_some_and(|(n, _)| n == name);
            if gone {
                removed += 1;
            }
            !gone
        })
        .map(|l| format!("{l}\n"))
        .collect();
    write_private(file, kept.as_bytes())?;
    Ok(removed)
}

fn write_private(file: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = file.with_extension("new");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, file).with_context(|| format!("writing {}", file.display()))?;
    Ok(())
}

async fn require_token(State(tokens): State<Tokens>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    match token.and_then(|t| tokens.check(&t)) {
        Some(name) => {
            req.extensions_mut().insert(AdminName(name));
            next.run(req).await
        }
        None => (StatusCode::UNAUTHORIZED, "Sign in again.\n").into_response(),
    }
}

async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    let set = |h: &mut axum::http::HeaderMap, k: header::HeaderName, v: &'static str| {
        h.insert(k, HeaderValue::from_static(v));
    };
    set(
        h,
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
    );
    set(h, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(h, header::REFERRER_POLICY, "no-referrer");
    set(h, header::X_FRAME_OPTIONS, "DENY");
    set(h, header::CACHE_CONTROL, "no-store");
    res
}

/// The admin router: `api` under `/admin/api` behind the token check, and the
/// GUI's files for everything else.
pub fn router(api: Router, tokens: Tokens, web_dir: &Path) -> Router {
    let api = api.layer(middleware::from_fn_with_state(tokens, require_token));
    let index = web_dir.join("index.html");
    Router::new()
        .nest("/admin/api", api)
        .fallback_service(ServeDir::new(web_dir).fallback(ServeFile::new(index)))
        .layer(middleware::from_fn(security_headers))
}

/// A page's built files, with the same security headers as the GUI: for the
/// families' account page, on the public server.
pub fn page(dir: &Path) -> Router {
    let index = dir.join("index.html");
    Router::new()
        .fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        .layer(middleware::from_fn(security_headers))
}

/// Serves the admin router: HTTPS with a certificate, otherwise plain HTTP,
/// which `AdminConfig` only allows on a loopback address.
pub async fn serve(cfg: &AdminConfig, router: Router) -> Result<()> {
    match &cfg.cert {
        Some(cert) => {
            let l = crate::TlsListener::bind(cfg.bind, cert, &[b"http/1.1"]).await?;
            tracing::info!("GUI on https://{}", cfg.bind);
            crate::serve_https(l, router).await
        }
        None => {
            anyhow::ensure!(
                cfg.bind.ip().is_loopback(),
                "the GUI needs a certificate to listen on {}; keep it on 127.0.0.1 otherwise",
                cfg.bind
            );
            let l = tokio::net::TcpListener::bind(cfg.bind)
                .await
                .with_context(|| format!("GUI on {}", cfg.bind))?;
            tracing::info!("GUI on http://{} (this server only)", cfg.bind);
            crate::serve_plain(l, router).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("admin-tokens");
        let a = new_token(&file, "suthan").expect("token");
        let b = new_token(&file, "ops-2").expect("token");
        let t = Tokens::new(file.clone());
        assert_eq!(t.check(&a).as_deref(), Some("suthan"));
        assert_eq!(t.check(&b).as_deref(), Some("ops-2"));
        assert_eq!(t.check("zat_nope"), None);
        assert!(!std::fs::read_to_string(&file).expect("read").contains(&a));
        assert_eq!(remove_tokens(&file, "suthan").expect("remove"), 1);
        assert_eq!(t.check(&a), None);
        assert_eq!(t.check(&b).as_deref(), Some("ops-2"));
        assert!(new_token(&file, "bad name").is_err());
    }
}
