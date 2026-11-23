//! Tor for Direct+ (docs/direct-plus.md §3), through Arti. No ZOOMRIYA
//! server: the box's own iroh relay waits behind an onion service, and the
//! apps reach it through Tor when no direct path works.
//!
//! | Side | What |
//! |---|---|
//! | The box | [`Tor::serve`]: an onion service with a stable address, forwarding port 80 to the box's iroh relay |
//! | The apps | [`proxy`]: a CONNECT proxy on 127.0.0.1 for iroh, reaching only the box's onion address |
//! | Networks that block Tor | [`Bridges`]: the family's own bridge lines, or the public ones when Tor cannot join within [`BRIDGES_AFTER`] and a transport program is installed |

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use arti_client::{
    BootstrapBehavior, TorClient, TorClientConfig,
    config::{
        BridgeConfigBuilder, CfgPath, Reconfigure, TorClientConfigBuilder,
        pt::TransportConfigBuilder,
    },
};
use futures::StreamExt;
use safelog::DisplayRedacted;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tor_cell::relaycell::msg::{Connected, End};
use tor_hsservice::{
    RunningOnionService, StreamRequest, config::OnionServiceConfigBuilder, handle_rend_requests,
};
use tor_rtcompat::PreferredRuntime;
use tracing::{debug, info};

/// The port the onion service answers on.
pub const ONION_PORT: u16 = 80;
/// Longest CONNECT request the proxy reads.
const MAX_REQUEST: usize = 4096;

/// How long Tor gets to join the network before trying the public bridges.
pub const BRIDGES_AFTER: Duration = Duration::from_secs(30);

/// The public bridges, for when Tor itself is blocked: Snowflake, from the
/// list Tor's own browser ships. Look again at each Arti update.
const PUBLIC_BRIDGES: &[&str] = &[
    "snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://snowflake-broker.torproject.net.global.prod.fastly.net/ front=cdn.sstatic.net ice=stun:stun.l.google.com:19302,stun:stun.antisip.com:3478,stun:stun.bluesip.net:3478,stun:stun.dus.net:3478,stun:stun.epygi.com:3478,stun:stun.sonetel.com:3478,stun:stun.uls.co.za:3478,stun:stun.voipgate.com:3478,stun:stun.voys.nl:3478 utls-imitate=hellorandomizedalpn",
];

/// The programs that carry each kind of bridge, by their usual names.
const TRANSPORTS: &[(&str, &[&str])] = &[
    ("snowflake", &["snowflake-client", "snowflake-pt-client"]),
    ("obfs4", &["lyrebird", "obfs4proxy"]),
    ("webtunnel", &["lyrebird"]),
];

/// Bridges into Tor, and the programs that carry them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bridges {
    /// Bridge lines, without the leading `Bridge`.
    pub lines: Vec<String>,
    /// For each kind of bridge in the lines: its program.
    pub transports: Vec<(String, PathBuf)>,
}

impl Bridges {
    /// Bridge lines as given: one per line, `Bridge` in front or not, `#`
    /// for notes. Each kind needs its program installed (`find` looks for it).
    pub fn parse(text: &str, find: impl Fn(&str) -> Option<PathBuf>) -> Self {
        let lines: Vec<String> = text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.strip_prefix("Bridge ").unwrap_or(l).trim().to_string())
            .collect();
        let mut transports: Vec<(String, PathBuf)> = Vec::new();
        for line in &lines {
            let kind = line.split_whitespace().next().unwrap_or_default();
            if kind.contains(['.', ':']) || transports.iter().any(|(k, _)| k == kind) {
                continue; // a plain bridge, or one already found
            }
            let program = TRANSPORTS
                .iter()
                .filter(|(k, _)| *k == kind)
                .flat_map(|(_, names)| names.iter())
                .find_map(|n| find(n));
            if let Some(p) = program {
                transports.push((kind.to_string(), p));
            }
        }
        Self { lines, transports }
    }

    /// The public bridges, if their program is installed.
    pub fn public(find: impl Fn(&str) -> Option<PathBuf>) -> Option<Self> {
        let b = Self::parse(&PUBLIC_BRIDGES.join("\n"), find);
        (!b.transports.is_empty()).then_some(b)
    }

    fn add_to(&self, b: &mut TorClientConfigBuilder) -> Result<()> {
        for line in &self.lines {
            let bridge: BridgeConfigBuilder = line.parse().context("a bridge line")?;
            b.bridges().bridges().push(bridge);
        }
        for (kind, program) in &self.transports {
            let mut t = TransportConfigBuilder::default();
            t.protocols(vec![kind.parse().context("a bridge kind")?])
                .path(CfgPath::new_literal(program.clone()))
                .run_on_startup(true);
            b.bridges().transports().push(t);
        }
        Ok(())
    }
}

/// A program on the PATH or beside this one, by name.
pub fn find_program(name: &str) -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    let path = std::env::var_os("PATH").unwrap_or_default();
    beside
        .into_iter()
        .chain(std::env::split_paths(&path))
        .flat_map(|d| [d.join(name), d.join(format!("{name}.exe"))])
        .find(|p| p.is_file())
}

fn tor_config(state: &Path, cache: &Path, bridges: Option<&Bridges>) -> Result<TorClientConfig> {
    let mut b = TorClientConfigBuilder::from_directories(state, cache);
    if let Some(br) = bridges {
        br.add_to(&mut b)?;
    }
    b.build().context("Tor settings")
}

/// A Tor client. Cheap to clone.
#[derive(Clone)]
pub struct Tor {
    client: Arc<TorClient<PreferredRuntime>>,
}

impl std::fmt::Debug for Tor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tor")
    }
}

impl Tor {
    /// A Tor client keeping its keys and state in `state` and what it
    /// downloads in `cache`. It joins the Tor network in the background, and
    /// anything asked of it waits until it has.
    ///
    /// With `bridges`, it goes through them from the start. Without, it
    /// moves to the public bridges if it has not joined within
    /// [`BRIDGES_AFTER`] and their program is installed.
    pub fn start(state: &Path, cache: &Path, bridges: Option<&Bridges>) -> Result<Self> {
        let config = tor_config(state, cache, bridges)?;
        let client = TorClient::builder()
            .config(config)
            .bootstrap_behavior(BootstrapBehavior::OnDemand)
            .create_unbootstrapped()
            .context("starting Tor")?;
        let c = client.clone();
        tokio::spawn(async move {
            match c.bootstrap().await {
                Ok(()) => info!("joined the Tor network"),
                Err(e) => info!("Tor: {e}"),
            }
        });
        if bridges.is_none() {
            let c = client.clone();
            let (state, cache) = (state.to_path_buf(), cache.to_path_buf());
            tokio::spawn(async move {
                tokio::time::sleep(BRIDGES_AFTER).await;
                if c.bootstrap_status().ready_for_traffic() {
                    return;
                }
                let Some(public) = Bridges::public(find_program) else {
                    debug!("Tor has not joined, and no bridge program is installed");
                    return;
                };
                info!("Tor has not joined: trying the public bridges");
                match tor_config(&state, &cache, Some(&public)) {
                    Ok(cfg) => {
                        if let Err(e) = c.reconfigure(&cfg, Reconfigure::WarnOnFailures) {
                            info!("Tor bridges: {e}");
                        }
                    }
                    Err(e) => info!("Tor bridges: {e:#}"),
                }
            });
        }
        Ok(Self { client })
    }

    /// An onion service named `nickname`, forwarding [`ONION_PORT`] to `to`.
    /// Its key is kept in the state folder, so the address never changes.
    pub fn serve(&self, nickname: &str, to: SocketAddr) -> Result<Onion> {
        let config = OnionServiceConfigBuilder::default()
            .nickname(nickname.parse().context("onion service name")?)
            .build()
            .context("onion service settings")?;
        let (service, requests) = self
            .client
            .launch_onion_service(config)
            .context("starting the onion service")?
            .context("onion service turned off")?;
        let address = service
            .onion_address()
            .context("onion service has no address")?
            .display_unredacted()
            .to_string();
        let task = tokio::spawn(async move {
            let mut streams = std::pin::pin!(handle_rend_requests(requests));
            while let Some(req) = streams.next().await {
                tokio::spawn(forward(req, to));
            }
        });
        info!("onion service started");
        Ok(Onion {
            address,
            _service: service,
            task,
        })
    }
}

/// A running onion service. Dropping it stops the service.
pub struct Onion {
    /// Its address, `….onion`.
    pub address: String,
    _service: Arc<RunningOnionService>,
    task: JoinHandle<()>,
}

impl Drop for Onion {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl std::fmt::Debug for Onion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Onion")
    }
}

async fn forward(req: StreamRequest, to: SocketAddr) {
    use tor_proto::stream::IncomingStreamRequest;
    let port = match req.request() {
        IncomingStreamRequest::Begin(b) => b.port(),
        _ => 0,
    };
    if port != ONION_PORT {
        let _ = req.reject(End::new_misc()).await;
        return;
    }
    let local = match TcpStream::connect(to).await {
        Ok(s) => s,
        Err(e) => {
            debug!("onion service: {e}");
            let _ = req.reject(End::new_misc()).await;
            return;
        }
    };
    let Ok(mut tor) = req.accept(Connected::new_empty()).await else {
        return;
    };
    let mut local = local;
    let _ = tokio::io::copy_bidirectional(&mut tor, &mut local).await;
}

/// A CONNECT proxy on 127.0.0.1 for iroh's relay client, reaching only
/// `onion` on [`ONION_PORT`] through Tor. Returns its address, to give iroh
/// as `http://<address>`.
pub async fn proxy(tor: Tor, onion: String) -> Result<(SocketAddr, JoinHandle<()>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let onion = Arc::new(onion);
    let task = tokio::spawn(async move {
        while let Ok((conn, _)) = listener.accept().await {
            let tor = tor.clone();
            let onion = onion.clone();
            tokio::spawn(async move {
                if let Err(e) = tunnel(conn, &tor, &onion).await {
                    debug!("Tor proxy: {e:#}");
                }
            });
        }
    });
    Ok((addr, task))
}

/// The target of a CONNECT request, if it is `onion` on [`ONION_PORT`].
fn connect_target(head: &str, onion: &str) -> bool {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some("CONNECT"), Some(target)) = (parts.next(), parts.next()) else {
        return false;
    };
    target.eq_ignore_ascii_case(&format!("{onion}:{ONION_PORT}"))
}

async fn tunnel(mut conn: TcpStream, tor: &Tor, onion: &str) -> Result<()> {
    let mut buf = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        anyhow::ensure!(buf.len() < MAX_REQUEST, "request too long");
        anyhow::ensure!(conn.read(&mut byte).await? == 1, "closed");
        buf.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&buf);
    if !connect_target(&head, onion) {
        conn.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        anyhow::bail!("refused a CONNECT elsewhere");
    }
    let mut stream = match tor.client.connect((onion, ONION_PORT)).await {
        Ok(s) => s,
        Err(e) => {
            conn.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await?;
            return Err(e.into());
        }
    };
    conn.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
    tokio::io::copy_bidirectional(&mut conn, &mut stream).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_lines() {
        let find = |n: &str| (n == "lyrebird").then(|| PathBuf::from("/usr/bin/lyrebird"));
        let b = Bridges::parse(
            "# mine\nBridge obfs4 192.0.2.55:38114 316E643333645F6D79216558614D3931657A5F5F cert=x iat-mode=0\n\nwebtunnel 192.0.2.9:443 AAAA url=https://example.com/x\nsnowflake 192.0.2.3:80 2B28\n192.0.2.1:9001\n",
            find,
        );
        assert_eq!(b.lines.len(), 4);
        assert!(b.lines[0].starts_with("obfs4 "));
        assert_eq!(
            b.transports,
            vec![
                ("obfs4".into(), PathBuf::from("/usr/bin/lyrebird")),
                ("webtunnel".into(), PathBuf::from("/usr/bin/lyrebird")),
            ]
        );
        assert!(Bridges::public(|_| None).is_none());
        let public = Bridges::public(|n| Some(PathBuf::from(n))).expect("public");
        let mut cfg = TorClientConfigBuilder::from_directories("/s", "/c");
        public.add_to(&mut cfg).expect("parses");
    }

    #[test]
    fn only_the_box() {
        let o = "abc.onion";
        assert!(connect_target(
            "CONNECT abc.onion:80 HTTP/1.1\r\nHost: x\r\n\r\n",
            o
        ));
        assert!(!connect_target("CONNECT abc.onion:443 HTTP/1.1\r\n\r\n", o));
        assert!(!connect_target(
            "CONNECT example.com:80 HTTP/1.1\r\n\r\n",
            o
        ));
        assert!(!connect_target("GET http://abc.onion/ HTTP/1.1\r\n\r\n", o));
    }
}
