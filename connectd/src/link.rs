//! connectd's side of iroh: pairing at home, and the family link from
//! anywhere. iroh chooses the paths; connectd decides who gets in, on every
//! path, from its own list. Away, devices reach the box at the addresses in
//! its DHT record, or through the box's own relay behind its onion service
//! (crate::relay, docs/direct-plus.md §3).

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use data_encoding::BASE64URL_NOPAD;
use iroh::{
    Endpoint, RelayMap, RelayMode, RelayUrl, TransportAddr,
    endpoint::{Connection, RecvStream, SendStream, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};
use zoomriya_keys::{
    PublicKey, SecretKey,
    pake::{self, Side},
    record::RecordSecret,
};
use zoomriya_proto::{
    laptop::{LaptopFinish, LaptopHello},
    link::{
        FromBox, LINK_ALPN, MAX_LINE, PAIR_ALPN, PairRequest, PairResponse, STREAM_CONTROL,
        STREAM_WEB, ToBox,
    },
};

use crate::{
    config::Target,
    devices::{Device, Devices, Refused},
};

/// The link's protocol version.
pub const VERSION: u32 = 1;

/// How the endpoint is set up.
#[derive(Debug, Clone)]
pub struct LinkConfig {
    /// The secret behind the box's DHT record, given to devices at pairing.
    pub record: Option<RecordSecret>,
    /// Where to listen.
    pub bind: Vec<SocketAddr>,
    /// The box's web servers.
    pub targets: Vec<Target>,
    /// Pairing over loopback: tests only.
    pub pairing_from_loopback: bool,
    /// The box's own iroh relay, on the box itself (crate::relay).
    pub relay: Option<RelayUrl>,
    /// The onion address in front of that relay, given to devices.
    pub onion: Option<String>,
}

/// Live connections, by link key, so a removed device is cut off at once.
#[derive(Debug, Default, Clone)]
pub struct Live(Arc<Mutex<HashMap<PublicKey, Vec<Connection>>>>);

impl Live {
    fn add(&self, key: PublicKey, conn: Connection) {
        if let Ok(mut m) = self.0.lock() {
            m.entry(key).or_default().push(conn);
        }
    }

    fn remove(&self, key: &PublicKey, conn: &Connection) {
        if let Ok(mut m) = self.0.lock()
            && let Some(v) = m.get_mut(key)
        {
            v.retain(|c| c.stable_id() != conn.stable_id());
            if v.is_empty() {
                m.remove(key);
            }
        }
    }

    /// Closes every connection from this link key.
    pub fn cut(&self, key: &PublicKey) -> usize {
        let conns = self
            .0
            .lock()
            .ok()
            .and_then(|mut m| m.remove(key))
            .unwrap_or_default();
        for c in &conns {
            c.close(1u32.into(), b"removed");
        }
        conns.len()
    }

    /// How many devices are connected now.
    pub fn count(&self) -> usize {
        self.0.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Whether this link key is connected now.
    pub fn is_connected(&self, key: &PublicKey) -> bool {
        self.0.lock().map(|m| m.contains_key(key)).unwrap_or(false)
    }
}

/// The running link.
pub struct Link {
    /// The router, holding the endpoint.
    pub router: Router,
    /// Live connections.
    pub live: Live,
}

impl Link {
    /// Binds the endpoint with the box key and starts accepting.
    pub async fn start(box_key: SecretKey, cfg: LinkConfig, devices: Arc<Devices>) -> Result<Self> {
        let mut b = Endpoint::builder(presets::Minimal)
            .secret_key(box_key)
            .alpns(vec![LINK_ALPN.to_vec(), PAIR_ALPN.to_vec()])
            .relay_mode(match &cfg.relay {
                Some(url) => RelayMode::Custom(RelayMap::from(url.clone())),
                None => RelayMode::Disabled,
            });
        for addr in &cfg.bind {
            b = b
                .bind_addr(*addr)
                .map_err(|e| anyhow::anyhow!("listening on {addr}: {e}"))?;
        }
        let endpoint = b.bind().await.context("starting iroh")?;
        let live = Live::default();
        let shared = Arc::new(Shared {
            endpoint: endpoint.clone(),
            devices,
            live: live.clone(),
            cfg,
        });
        let router = Router::builder(endpoint)
            .accept(LINK_ALPN, LinkHandler(shared.clone()))
            .accept(PAIR_ALPN, PairHandler(shared))
            .spawn();
        info!(id = %router.endpoint().id().fmt_short(), "link listening");
        Ok(Self { router, live })
    }
}

#[derive(Debug)]
struct Shared {
    endpoint: Endpoint,
    devices: Arc<Devices>,
    live: Live,
    cfg: LinkConfig,
}

impl Shared {
    /// Where devices can reach the box directly: every IP address iroh knows
    /// for it.
    fn direct_addrs(&self) -> Vec<SocketAddr> {
        self.endpoint
            .addr()
            .addrs
            .iter()
            .filter_map(|a| match a {
                TransportAddr::Ip(s) => Some(*s),
                _ => None,
            })
            .collect()
    }
}

fn err(e: impl std::fmt::Display) -> AcceptError {
    AcceptError::from_err(std::io::Error::other(e.to_string()))
}

async fn read_line<R: AsyncRead + Unpin>(r: &mut BufReader<R>) -> Result<Option<String>> {
    let mut line = Vec::new();
    let n = (&mut *r)
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut line)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    anyhow::ensure!(line.len() <= MAX_LINE, "line too long");
    Ok(Some(String::from_utf8(line)?.trim_end().to_string()))
}

async fn write_line<W: AsyncWrite + Unpin, T: serde::Serialize>(w: &mut W, v: &T) -> Result<()> {
    let mut s = serde_json::to_vec(v)?;
    s.push(b'\n');
    w.write_all(&s).await?;
    Ok(())
}

// Pairing.

#[derive(Debug, Clone)]
struct PairHandler(Arc<Shared>);

/// Whether the connection runs directly over the home network.
fn from_home(conn: &Connection, loopback_ok: bool) -> bool {
    conn.paths().iter().any(|p| match p.remote_addr() {
        TransportAddr::Ip(s) => match s.ip() {
            IpAddr::V4(a) => {
                a.is_private() || a.is_link_local() || (loopback_ok && a.is_loopback())
            }
            IpAddr::V6(a) => {
                zoomriya_proto::addrs::is_home(IpAddr::V6(a))
                    || (a.segments()[0] & 0xffc0) == 0xfe80
                    || (loopback_ok && a.is_loopback())
            }
        },
        _ => false,
    })
}

/// How long a laptop has for its last pairing step.
const LAPTOP_STEP: Duration = Duration::from_secs(60);

fn outcome(r: Result<(Device, Vec<SocketAddr>), Refused>, cfg: &LinkConfig) -> PairResponse {
    match r {
        Ok((device, addrs)) => {
            info!(
                device = %device.name,
                admin = device.admin,
                laptop = device.laptop,
                "paired"
            );
            PairResponse::Paired {
                addrs,
                record: cfg.record.as_ref().map(RecordSecret::to_b64),
                onion: cfg.onion.clone(),
            }
        }
        Err(why) => {
            warn!(%why, "pairing refused");
            PairResponse::Refused {
                code: why.code().into(),
                message: why.message().into(),
            }
        }
    }
}

/// A laptop's pairing after its hello: one try used, the box's SPAKE2
/// message and confirmation sent, the laptop's checked
/// (zoomriya_keys::pake). A wrong code fails either side's confirmation.
async fn pair_laptop<R: AsyncRead + Unpin>(
    s: &Shared,
    from: &PublicKey,
    hello: &LaptopHello,
    send: &mut SendStream,
    recv: &mut BufReader<R>,
) -> Result<(Device, Vec<SocketAddr>), Refused> {
    let (code, name) = s.devices.laptop_attempt(crate::now())?;
    let theirs = BASE64URL_NOPAD
        .decode(hello.spake.as_bytes())
        .map_err(|_| Refused::PairingCode)?;
    let start = pake::start(&code);
    let box_msg = start.msg.clone();
    let t = pake::transcript(&s.endpoint.id(), from, &theirs, &box_msg);
    let session = start.finish(&theirs).map_err(|_| Refused::PairingCode)?;
    let challenge = PairResponse::Challenge {
        spake: BASE64URL_NOPAD.encode(&box_msg),
        confirm: BASE64URL_NOPAD.encode(&session.confirm(Side::Box, &t, b"")),
    };
    write_line(send, &challenge)
        .await
        .map_err(|_| Refused::PairingCode)?;
    let line = tokio::time::timeout(LAPTOP_STEP, read_line(recv))
        .await
        .ok()
        .and_then(|l| l.ok().flatten())
        .ok_or(Refused::PairingCode)?;
    let finish: LaptopFinish = serde_json::from_str(&line).map_err(|_| Refused::PairingCode)?;
    let confirm = BASE64URL_NOPAD
        .decode(finish.confirm.as_bytes())
        .map_err(|_| Refused::PairingCode)?;
    session
        .check(Side::Laptop, &t, &finish.extra(), &confirm)
        .map_err(|_| Refused::PairingCode)?;
    let device = s
        .devices
        .finish_laptop(&finish, from, &name, crate::now())?;
    Ok((device, s.direct_addrs()))
}

impl ProtocolHandler for PairHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let s = &self.0;
        let from = conn.remote_id();
        // Pairing happens only at home, over the home network.
        if !from_home(&conn, s.cfg.pairing_from_loopback) {
            warn!(from = %from.fmt_short(), "pairing from away refused");
            conn.close(2u32.into(), b"pair at home");
            return Ok(());
        }
        let (mut send, recv) = conn.accept_bi().await.map_err(err)?;
        let mut recv = BufReader::new(recv);
        let line = read_line(&mut recv).await.map_err(err)?.unwrap_or_default();
        // A phone sends the QR code's secret; a laptop proves it knows the
        // short code typed into the laptop app, in three steps.
        let answer = if let Ok(req) = serde_json::from_str::<PairRequest>(&line) {
            outcome(
                s.devices
                    .pair(&req, &from, crate::now())
                    .map(|d| (d, s.direct_addrs())),
                &s.cfg,
            )
        } else if let Ok(hello) = serde_json::from_str::<LaptopHello>(&line) {
            outcome(
                pair_laptop(s, &from, &hello, &mut send, &mut recv).await,
                &s.cfg,
            )
        } else {
            PairResponse::Refused {
                code: "bad_request".into(),
                message: "Pairing didn't work. Update the ZOOMRIYA app and try again.".into(),
            }
        };
        write_line(&mut send, &answer).await.map_err(err)?;
        send.finish().map_err(err)?;
        conn.closed().await;
        Ok(())
    }
}

// The family link.

#[derive(Debug, Clone)]
struct LinkHandler(Arc<Shared>);

impl ProtocolHandler for LinkHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let s = self.0.clone();
        let link = conn.remote_id();
        let Some(device) = s.devices.by_link(&link) else {
            debug!(from = %link.fmt_short(), "not on the list");
            conn.close(1u32.into(), b"not paired");
            return Ok(());
        };
        s.devices.seen(&device.id, crate::now());
        s.live.add(link, conn.clone());
        let result = serve_link(&s, &conn, &device).await;
        s.live.remove(&link, &conn);
        if let Err(e) = result {
            debug!(device = %device.name, "link ended: {e:#}");
        }
        Ok(())
    }
}

async fn serve_link(s: &Arc<Shared>, conn: &Connection, device: &Device) -> Result<()> {
    loop {
        let (send, mut recv) = match conn.accept_bi().await {
            Ok(st) => st,
            Err(_) => return Ok(()),
        };
        // The list is checked on every stream, not only at the start.
        if s.devices.by_link(&conn.remote_id()).is_none() {
            conn.close(1u32.into(), b"removed");
            return Ok(());
        }
        let kind = recv.read_u8().await?;
        let s = s.clone();
        let device = device.clone();
        tokio::spawn(async move {
            let r = match kind {
                STREAM_CONTROL => control(&s, &device, send, recv).await,
                STREAM_WEB => web(&s, &device, send, recv).await,
                _ => Err(anyhow::anyhow!("unknown stream {kind}")),
            };
            if let Err(e) = r {
                debug!(device = %device.name, "stream: {e:#}");
            }
        });
    }
}

async fn control(
    s: &Shared,
    device: &Device,
    mut send: SendStream,
    recv: RecvStream,
) -> Result<()> {
    let mut recv = BufReader::new(recv);
    while let Some(line) = read_line(&mut recv).await? {
        let Ok(msg) = serde_json::from_str::<ToBox>(&line) else {
            continue;
        };
        let answer = match msg {
            ToBox::Hello { version, .. } if version < VERSION => Some(FromBox::Refused {
                code: "please_update".into(),
                message: "Update the ZOOMRIYA app to keep using your box away from home.".into(),
            }),
            ToBox::Hello { .. } => Some(FromBox::Welcome {
                addrs: s.direct_addrs(),
            }),
            ToBox::Report(r) => {
                s.devices.report(&device.id, r);
                None
            }
        };
        if let Some(a) = answer {
            write_line(&mut send, &a).await?;
        }
    }
    Ok(())
}

/// A web stream: on to one of the box's own web servers, labelled with the
/// device, if the target takes labels (PROXY protocol v2).
async fn web(
    s: &Shared,
    device: &Device,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<()> {
    let len = recv.read_u8().await? as usize;
    let mut name = vec![0u8; len];
    recv.read_exact(&mut name).await?;
    let name = String::from_utf8(name)?;
    let target = s
        .cfg
        .targets
        .iter()
        .find(|t| t.name == name)
        .with_context(|| format!("no target {name}"))?;
    let header = if target.proxy_protocol {
        proxy_v2_header(device)
    } else {
        Vec::new()
    };
    match (&target.unix, target.tcp) {
        (Some(path), _) => {
            let mut up = tokio::net::UnixStream::connect(path)
                .await
                .with_context(|| format!("reaching {}", path.display()))?;
            up.write_all(&header).await?;
            pipe(&mut up, &mut send, &mut recv).await
        }
        (None, Some(addr)) => {
            let mut up = tokio::net::TcpStream::connect(addr)
                .await
                .with_context(|| format!("reaching {addr}"))?;
            up.write_all(&header).await?;
            pipe(&mut up, &mut send, &mut recv).await
        }
        _ => Ok(()),
    }
}

async fn pipe<S: AsyncRead + AsyncWrite + Unpin>(
    up: &mut S,
    send: &mut SendStream,
    recv: &mut RecvStream,
) -> Result<()> {
    let (mut ur, mut uw) = tokio::io::split(up);
    let to_up = async {
        tokio::io::copy(recv, &mut uw).await?;
        uw.shutdown().await
    };
    let to_device = async {
        tokio::io::copy(&mut ur, send).await?;
        send.finish().map_err(std::io::Error::other)
    };
    let (a, b) = tokio::join!(to_up, to_device);
    a?;
    b?;
    Ok(())
}

/// The PROXY protocol v2 signature.
pub const PROXY_SIGNATURE: [u8; 12] = *b"\r\n\r\n\0\r\nQUIT\n";
/// TLV: the device's ID on the box.
pub const TLV_DEVICE_ID: u8 = 0xE0;
/// TLV: the device's name, such as "Mum's phone", for the activity log.
pub const TLV_DEVICE_NAME: u8 = 0xE1;

/// A PROXY v2 header carrying the device. The address family is "unspecified"
/// (the connection is local); who it is travels in our own TLVs, which the
/// gateway accepts only on connectd's socket.
pub fn proxy_v2_header(device: &Device) -> Vec<u8> {
    let mut tlvs = Vec::new();
    for (t, v) in [
        (TLV_DEVICE_ID, device.id.as_bytes()),
        (TLV_DEVICE_NAME, device.name.as_bytes()),
    ] {
        let v = &v[..v.len().min(255)];
        tlvs.push(t);
        tlvs.extend_from_slice(&(v.len() as u16).to_be_bytes());
        tlvs.extend_from_slice(v);
    }
    let mut h = PROXY_SIGNATURE.to_vec();
    h.push(0x21); // version 2, PROXY
    h.push(0x00); // unspecified family and protocol
    h.extend_from_slice(&(tlvs.len() as u16).to_be_bytes());
    h.extend_from_slice(&tlvs);
    h
}

/// The data directory's devices file.
pub fn devices_file(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("devices.json")
}
