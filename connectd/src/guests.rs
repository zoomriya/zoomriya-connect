//! Guests (docs/direct-plus.md §12): they have only a browser, and get only
//! Photos' album viewer, which the apps' gateway serves on its own site
//! (zoomriya-apps platform/gateway/Caddyfile, `(guests)`).
//!
//! | Way in | When | How |
//! |---|---|---|
//! | The door, `https://share.home.<domain>:8443` | The family has its own name | connectd asks the router (UPnP) to open TCP 8443 to the box; over IPv6 the router's firewall must let it in |
//! | Otherwise | Always | Guest links work at home; away, the family sends the photos |
//!
//! The door is checked from outside, through Tor, as a stranger would see
//! it: the viewer answers, and nothing else does.
//!
//! The door's address, once it works, goes to `guests.json` beside the web
//! servers' sockets (group zoomriya-link), which the gateway serves to the
//! family's pages for Photos' links.

use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info, warn};
use zoomriya_tor::Tor;

use crate::daemon::Ctx;

/// The door's port, inside and out: the name's IPv4 and IPv6 addresses
/// share it.
pub const DOOR_PORT: u16 = 8443;
/// How long the router keeps the door open; renewed well before.
const LEASE: u32 = 3600;
/// How often the door is looked at.
const LOOK: Duration = Duration::from_secs(20 * 60);
/// How long a check through Tor may take.
const CHECK: Duration = Duration::from_secs(90);

/// What Box Desktop shows about guests.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GuestsStatus {
    /// The door's address, when the family has its own name.
    pub door: Option<String>,
    /// The router opened TCP 8443 for the box.
    pub door_mapped: bool,
    /// From outside, through Tor: the viewer answers.
    pub door_reachable: Option<bool>,
    /// From outside: only the viewer answers.
    pub door_only_viewer: Option<bool>,
    /// When it was last checked from outside.
    pub checked_at: Option<u64>,
    /// What went wrong last.
    pub problem: Option<String>,
}

/// Guests' state, shared with the API.
#[derive(Debug, Default)]
pub struct Guests(Mutex<GuestsStatus>);

impl Guests {
    /// What Box Desktop shows.
    pub fn status(&self) -> GuestsStatus {
        self.0.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut GuestsStatus)) {
        if let Ok(mut s) = self.0.lock() {
            f(&mut s);
        }
    }
}

/// What Photos reads to make guest links.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct ForPhotos {
    door: Option<String>,
}

/// Writes `guests.json` for the gateway's group to read, whole or not at all.
fn write_for_photos(file: &Path, f: &ForPhotos) -> Result<()> {
    let tmp = file.with_extension("new");
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o640);
        }
        let mut out = opts
            .open(&tmp)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::io::Write::write_all(&mut out, &serde_json::to_vec(f)?)?;
    }
    std::fs::rename(&tmp, file).with_context(|| format!("writing {}", file.display()))
}

/// The box's own address towards the router.
fn local_towards(router: SocketAddr) -> Result<IpAddr> {
    let s = std::net::UdpSocket::bind(("0.0.0.0", 0))?;
    s.connect(router)?;
    Ok(s.local_addr()?.ip())
}

fn search_options() -> igd_next::SearchOptions {
    let mut o = igd_next::SearchOptions::default();
    o.timeout = Some(Duration::from_secs(5));
    o
}

/// Asks the router to open the door.
async fn open_door() -> Result<()> {
    use igd_next::{PortMappingProtocol, aio::tokio::search_gateway};
    let gw = search_gateway(search_options())
        .await
        .context("no router answered for opening ports (UPnP)")?;
    let local = local_towards(gw.addr)?;
    gw.add_port(
        PortMappingProtocol::TCP,
        DOOR_PORT,
        SocketAddr::new(local, DOOR_PORT),
        LEASE,
        "ZOOMRIYA guests",
    )
    .await
    .context("the router refused to open the guests' port")?;
    Ok(())
}

async fn close_door() {
    use igd_next::{PortMappingProtocol, aio::tokio::search_gateway};
    if let Ok(gw) = search_gateway(search_options()).await {
        let _ = gw.remove_port(PortMappingProtocol::TCP, DOOR_PORT).await;
    }
}

/// The status code `path` gets at `host`:`port`, through Tor, over HTTPS
/// checked against the public authorities.
async fn status_through_tor(tor: &Tor, host: &str, port: u16, path: &str) -> Result<u16> {
    use std::sync::Arc as A;
    let stream = tor.connect(host, port).await?;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string())?;
    let mut tls = tokio_rustls::TlsConnector::from(A::new(cfg))
        .connect(name, stream)
        .await
        .context("the door's certificate")?;
    tls.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await?;
    let mut head = [0u8; 12];
    tls.read_exact(&mut head).await?;
    let line = String::from_utf8_lossy(&head);
    line.get(9..12)
        .and_then(|c| c.parse().ok())
        .context("not an HTTP answer")
}

/// Checks the door as a stranger: the viewer answers, a family page does not.
async fn check_from_outside(tor: &Tor, host: &str) -> (Option<bool>, Option<bool>) {
    let viewer = tokio::time::timeout(
        CHECK,
        status_through_tor(tor, host, DOOR_PORT, "/photos/view/check"),
    )
    .await;
    let reachable = match viewer {
        Ok(Ok(code)) => Some(code == 200),
        Ok(Err(e)) => {
            debug!("the door from outside: {e:#}");
            Some(false)
        }
        Err(_) => Some(false),
    };
    if reachable != Some(true) {
        return (reachable, None);
    }
    let family = tokio::time::timeout(
        CHECK,
        status_through_tor(tor, host, DOOR_PORT, "/id/api/v1/users/me"),
    )
    .await;
    let only_viewer = match family {
        Ok(Ok(code)) => Some(code == 404),
        _ => None,
    };
    (reachable, only_viewer)
}

/// Keeps the door and `guests.json` current, until stopped.
pub async fn keep(ctx: Arc<Ctx>, tor: Option<Tor>) {
    let file = ctx.cfg.guests_file.clone();
    let mut last: Option<ForPhotos> = None;
    let mut was_open = false;
    loop {
        let names = ctx.names.settings();
        let wanted = ctx.away.on() && names.is_some();
        let door = names
            .as_ref()
            .map(|n| format!("https://{}:{DOOR_PORT}", n.share()));
        if wanted {
            match open_door().await {
                Ok(()) => {
                    if !was_open {
                        info!("the guests' door is open");
                    }
                    was_open = true;
                    ctx.guests.update(|s| {
                        s.door_mapped = true;
                        s.problem = None;
                    });
                }
                Err(e) => {
                    debug!("the guests' door: {e:#}");
                    was_open = false;
                    ctx.guests.update(|s| {
                        s.door_mapped = false;
                        s.problem = Some(format!("{e:#}"));
                    });
                }
            }
            // A certificate is needed before strangers can be shown anything.
            if let (Some(tor), Some(n)) = (&tor, &names)
                && ctx.names.status().certificate_days.is_some()
            {
                let (reachable, only_viewer) = check_from_outside(tor, &n.share()).await;
                if only_viewer == Some(false) {
                    warn!("from outside, more than the album viewer answers at the guests' door");
                }
                ctx.guests.update(|s| {
                    s.door_reachable = reachable;
                    s.door_only_viewer = only_viewer;
                    s.checked_at = Some(crate::now());
                });
            }
        } else if was_open {
            close_door().await;
            was_open = false;
            info!("the guests' door is closed");
        }
        let reachable = ctx.guests.status().door_reachable == Some(true);
        ctx.guests.update(|s| {
            s.door = door.clone().filter(|_| wanted);
            if !wanted {
                s.door_mapped = false;
                s.door_reachable = None;
                s.door_only_viewer = None;
            }
        });
        let now = ForPhotos {
            door: door.filter(|_| wanted && reachable),
        };
        if last.as_ref() != Some(&now) {
            match write_for_photos(&file, &now) {
                Ok(()) => last = Some(now),
                Err(e) => debug!("{e:#}"),
            }
        }
        tokio::time::sleep(LOOK).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photos_reads_the_links_and_the_gateway_may_too() {
        let dir = tempfile::tempdir().expect("dir");
        let f = dir.path().join("guests.json");
        let g = ForPhotos {
            door: Some("https://share.home.smith.ca:8443".into()),
        };
        write_for_photos(&f, &g).expect("write");
        let back: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&f).expect("read")).expect("json");
        assert_eq!(back["door"], "https://share.home.smith.ca:8443");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&f).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }
    }

    #[test]
    fn the_box_address_towards_the_router() {
        let ip = local_towards("127.0.0.1:1900".parse().expect("addr")).expect("local");
        assert!(ip.is_loopback());
    }
}
