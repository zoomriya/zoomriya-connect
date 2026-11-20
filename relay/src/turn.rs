//! The TURN server beside this relay (coturn, `deploy/turnserver.conf`):
//! laptops fall back to it when WebRTC can't connect directly
//! (docs/laptops.md, "When the direct path fails").
//!
//! | What | How |
//! |---|---|
//! | Credentials | `POST /zoomriya/v1/turn`, signed with the box key: TURN's shared-secret kind, the username `<expiry>:<box key>`, so coturn's records name the box |
//! | Counting | coturn writes a `usage:` line per session every few thousand packets and when it ends (`verbose` on). We read its log and add each box's bytes to the next report |
//! | Port 443 | TURN over TLS on `turn.<domain>`: the front door opens the TLS with the relay's own certificate and hands the TURN stream to coturn's TCP port on this machine (`front.rs`) |
//! | Health | A STUN request to coturn every report; the GUI shows whether it answers |

use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use data_encoding::BASE64;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use tokio::net::UdpSocket;
use tracing::{debug, warn};
use zoomriya_keys::{PublicKey, REQUEST_SIGNATURE_HEADER, RequestSignature, key_from_str};
use zoomriya_proto::{RELAY_TURN, TurnCredentials, stun};

use crate::{
    config,
    front::{Api, error},
};

/// The TURN server beside this relay.
pub struct Turn {
    cfg: config::Turn,
    secret: String,
    usage: Mutex<HashMap<PublicKey, u64>>,
    up: AtomicBool,
    total: std::sync::atomic::AtomicU64,
    /// Whether each box's bytes are kept for the control plane's report.
    report: bool,
}

impl std::fmt::Debug for Turn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Turn")
    }
}

impl Turn {
    /// Reads the shared secret. `report`: this relay reports each box's use
    /// to a control plane.
    pub fn new(cfg: config::Turn, report: bool) -> anyhow::Result<Self> {
        let secret = std::fs::read_to_string(&cfg.secret_file)
            .map_err(|e| anyhow::anyhow!("{}: {e}", cfg.secret_file.display()))?
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        anyhow::ensure!(
            secret.len() >= 32,
            "{}: at least 32 characters",
            cfg.secret_file.display()
        );
        Ok(Self {
            cfg,
            secret,
            usage: Mutex::default(),
            up: AtomicBool::new(false),
            total: std::sync::atomic::AtomicU64::new(0),
            report,
        })
    }

    /// The name TURN over TLS uses on port 443, and coturn's address here.
    pub fn route(&self) -> (String, SocketAddr) {
        (self.cfg.name.clone(), self.cfg.local)
    }

    /// Credentials for one box, from `now`.
    pub fn credentials(&self, box_key: &PublicKey, now: u64) -> Option<TurnCredentials> {
        let username = format!("{}:{}", now + self.cfg.ttl_seconds, box_key.to_z32());
        let mut mac = Hmac::<Sha1>::new_from_slice(self.secret.as_bytes()).ok()?;
        mac.update(username.as_bytes());
        Some(TurnCredentials {
            urls: self.cfg.urls.clone(),
            credential: BASE64.encode(&mac.finalize().into_bytes()),
            username,
            ttl: self.cfg.ttl_seconds,
        })
    }

    /// Whether coturn answered the last check.
    pub fn up(&self) -> bool {
        self.up.load(Ordering::Relaxed)
    }

    /// Each box's bytes since the last call.
    pub fn take(&self) -> Vec<(PublicKey, u64)> {
        self.usage
            .lock()
            .map(|mut u| u.drain().collect())
            .unwrap_or_default()
    }

    /// Counts that didn't get through, for the next report.
    pub fn put_back(&self, used: Vec<(PublicKey, u64)>) {
        if let Ok(mut u) = self.usage.lock() {
            for (k, b) in used {
                *u.entry(k).or_default() += b;
            }
        }
    }

    /// Every box's bytes since the relay started, for its own GUI.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    fn add(&self, key: PublicKey, bytes: u64) {
        self.total.fetch_add(bytes, Ordering::Relaxed);
        // Only a relay that reports to a control plane takes these; a
        // family's own keeps the total alone.
        if !self.report {
            return;
        }
        if let Ok(mut u) = self.usage.lock() {
            *u.entry(key).or_default() += bytes;
        }
    }

    /// Checks coturn answers STUN.
    pub async fn check(&self) {
        let up = probe(self.cfg.local).await;
        if up != self.up.swap(up, Ordering::Relaxed) {
            if up {
                tracing::info!(addr = %self.cfg.local, "TURN server answering");
            } else {
                warn!(addr = %self.cfg.local, "TURN server not answering");
            }
        }
    }

    /// Reads coturn's log as it grows, forever: each `usage:` line's bytes
    /// go to its box. Starts at the end, and starts again from the top of a
    /// new file when coturn's log is rotated.
    pub async fn follow_log(&self) {
        let Some(path) = self.cfg.log.clone() else {
            return;
        };
        let mut tail = Tail::new(path);
        loop {
            for line in tail.lines() {
                if let Some((key, bytes)) = usage_line(&line) {
                    self.add(key, bytes);
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn probe(addr: SocketAddr) -> bool {
    let bind = if addr.is_ipv4() {
        SocketAddr::from(([0, 0, 0, 0], 0))
    } else {
        SocketAddr::from(([0u16; 8], 0))
    };
    let Ok(sock) = UdpSocket::bind(bind).await else {
        return false;
    };
    let mut txid = [0u8; 12];
    if getrandom::fill(&mut txid).is_err() {
        return false;
    }
    if sock.send_to(&stun::request(&txid), addr).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_secs(2), sock.recv_from(&mut buf)).await {
        Ok(Ok((n, _))) => stun::mapped(&buf[..n], &txid).is_some(),
        _ => false,
    }
}

/// A box's bytes from one of coturn's `usage:` lines:
/// `session 001000000000000001: usage: realm=<turn.zoomriya.net>,
/// username=<1795000000:<box key>>, rp=10, rb=1200, sp=9, sb=900`. Each line
/// counts what moved since the session's last line.
pub fn usage_line(line: &str) -> Option<(PublicKey, u64)> {
    let rest = line.split_once(": usage: ")?.1;
    let user = rest.split_once("username=<")?.1;
    let (user, after) = user.split_once(">,")?;
    let key = user.split_once(':')?.1;
    let key = key_from_str(key.split('.').next()?).ok()?;
    let field = |name: &str| -> u64 {
        after
            .split(',')
            .find_map(|f| f.trim().strip_prefix(name))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    let bytes = field("rb=").saturating_add(field("sb="));
    (bytes > 0).then_some((key, bytes))
}

/// Reads a log file as it grows.
struct Tail {
    path: PathBuf,
    pos: u64,
    inode: u64,
    started: bool,
    partial: String,
}

impl Tail {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            pos: 0,
            inode: 0,
            started: false,
            partial: String::new(),
        }
    }

    /// Whole lines added since the last call.
    fn lines(&mut self) -> Vec<String> {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return vec![];
        };
        let Ok(meta) = f.metadata() else {
            return vec![];
        };
        if !self.started {
            // What was there before we started was counted by the last run.
            self.started = true;
            self.inode = meta.ino();
            self.pos = meta.len();
            return vec![];
        }
        if meta.ino() != self.inode || meta.len() < self.pos {
            // Rotated: the new file from its start.
            self.inode = meta.ino();
            self.pos = 0;
            self.partial.clear();
        }
        if meta.len() == self.pos || f.seek(SeekFrom::Start(self.pos)).is_err() {
            return vec![];
        }
        let mut bytes = Vec::new();
        // At most 8 MB a turn: a huge backlog is read over several.
        if f.take(8 << 20).read_to_end(&mut bytes).is_err() {
            return vec![];
        }
        self.pos += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        let mut out: Vec<String> = Vec::new();
        while let Some(i) = self.partial.find('\n') {
            out.push(self.partial[..i].to_string());
            self.partial.drain(..=i);
        }
        out
    }
}

/// `POST /zoomriya/v1/turn`: a box asks for credentials, signed with its key.
pub async fn credentials(State(api): State<Api>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(turn) = &api.turn else {
        return error(
            StatusCode::NOT_FOUND,
            "no_turn",
            "This relay has no TURN server.",
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
    if sig
        .verify(
            "POST",
            RELAY_TURN,
            &body,
            zoomriya_serve::now(),
            api.clock_allowance,
        )
        .is_err()
    {
        return error(
            StatusCode::UNAUTHORIZED,
            "unsigned",
            "The signature doesn't match.",
        );
    }
    match api.gate.decide(&sig.key, None) {
        Ok(k) if k == sig.key => {}
        _ => {
            return error(
                StatusCode::FORBIDDEN,
                "unknown_box",
                "This relay doesn't know your box.",
            );
        }
    }
    match turn.credentials(&sig.key, zoomriya_serve::now()) {
        Some(c) => {
            debug!(box_key = %sig.key.fmt_short(), "TURN credentials");
            Json(c).into_response()
        }
        None => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Something went wrong on our side.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zoomriya_keys::SecretKey;

    #[test]
    fn coturns_usage_lines() {
        let k = SecretKey::generate().public();
        let line = format!(
            "1795000000: : session 001000000000000001: usage: realm=<turn.zoomriya.net>, username=<1795003600:{}>, rp=10, rb=1200, sp=9, sb=900",
            k.to_z32()
        );
        assert_eq!(usage_line(&line), Some((k, 2100)));
        let peer = line
            .replace(": usage: ", ": peer usage: ")
            .replace("username=", "user=");
        assert_eq!(usage_line(&peer), None, "peer lines aren't counted twice");
        assert_eq!(
            usage_line("session 1: usage: realm=<x>, username=<guest>, rp=1, rb=5, sp=1, sb=5"),
            None
        );
    }

    #[test]
    fn tail_reads_new_lines_and_rotations() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("turn.log");
        std::fs::write(&path, "old line\n").expect("write");
        let mut t = Tail::new(path.clone());
        assert!(t.lines().is_empty(), "starts at the end");
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        write!(f, "one\ntw").expect("write");
        assert_eq!(t.lines(), vec!["one"]);
        writeln!(f, "o").expect("write");
        assert_eq!(t.lines(), vec!["two"]);
        // Rotated: a new file.
        std::fs::remove_file(&path).expect("remove");
        std::fs::write(&path, "fresh\n").expect("write");
        assert_eq!(t.lines(), vec!["fresh"]);
    }

    #[test]
    fn credentials_name_the_box() {
        let dir = tempfile::tempdir().expect("dir");
        let secret = dir.path().join("secret");
        std::fs::write(&secret, "0123456789abcdef0123456789abcdef\n").expect("write");
        let turn = Turn::new(
            config::Turn {
                name: "turn.zoomriya.net".into(),
                local: "127.0.0.1:3478".parse().expect("addr"),
                urls: vec!["turn:turn.zoomriya.net:3478?transport=udp".into()],
                secret_file: secret,
                ttl_seconds: 86_400,
                log: None,
            },
            true,
        )
        .expect("turn");
        let k = SecretKey::generate().public();
        let c = turn.credentials(&k, 1000).expect("credentials");
        assert_eq!(c.username, format!("87400:{}", k.to_z32()));
        // coturn's check: base64(HMAC-SHA1(secret, username)).
        let mut mac =
            Hmac::<Sha1>::new_from_slice(b"0123456789abcdef0123456789abcdef").expect("mac");
        mac.update(c.username.as_bytes());
        assert_eq!(c.credential, BASE64.encode(&mac.finalize().into_bytes()));
    }
}
