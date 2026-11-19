//! connectd's settings: `/etc/zoomriya-connect/connectd.toml`
//! (`deploy/connectd.toml` in this repository).

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Everything in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The box key, the DNS update key, the device list. Never in backups
    /// (zoomriya-os docs/design.md §4).
    #[serde(default = "data_dir")]
    pub data_dir: PathBuf,
    /// Where connectd leaves its certificate, `connect.pem`. On ZOOMRIYA OS
    /// the agent checks it and puts it beside the box's own certificate for
    /// the gateway and Box Desktop: connectd itself can't write there.
    #[serde(default = "certs_dir")]
    pub certs_dir: PathBuf,
    /// The ZOOMRIYA Connect server, unless Box Desktop set another.
    #[serde(default = "server")]
    pub server: String,
    /// The UDP port iroh listens on: 443, so a forwarded port is easy.
    #[serde(default = "port")]
    pub port: u16,
    /// Box Desktop's way in: a Unix socket only box-desktopd's group can use.
    #[serde(default = "api_socket")]
    pub api_socket: PathBuf,
    /// A file holding a ready-made box's factory token, if it has one.
    #[serde(default = "factory_token")]
    pub factory_token_file: PathBuf,
    /// Laptops' WebRTC and their pairing at home.
    #[serde(default)]
    pub laptops: Laptops,
    /// The box's own web servers that paired devices reach.
    #[serde(default = "targets")]
    pub targets: Vec<Target>,
    /// Certificate authorities, tried in order.
    #[serde(default = "authorities")]
    pub certificate_authorities: Vec<Authority>,
    /// A certificate authority to trust beside the usual ones, for a family's
    /// own server whose certificates come from its own authority.
    #[serde(default)]
    pub extra_ca_file: Option<PathBuf>,
    /// Trust the clock without systemd-timesyncd: for development only.
    #[serde(default)]
    pub trust_clock: bool,
}

/// One of the box's web servers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// The name devices ask for: `gateway`, `box-desktop`.
    pub name: String,
    /// A Unix socket only connectd can use.
    #[serde(default)]
    pub unix: Option<PathBuf>,
    /// Or an address on the box itself.
    #[serde(default)]
    pub tcp: Option<SocketAddr>,
    /// Label each connection with its device (PROXY protocol v2). Only on a
    /// socket nobody else can reach, never on the home network's port.
    #[serde(default)]
    pub proxy_protocol: bool,
    /// The server speaks TLS (it does on the box). Laptops' requests reach it
    /// as plain HTTP inside WebRTC, so connectd opens the TLS itself.
    #[serde(default = "yes")]
    pub tls: bool,
}

fn yes() -> bool {
    true
}

/// Laptops: the browser extension, over WebRTC (docs/laptops.md).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Laptops {
    /// TCP, the home network only: pairing and setting up connections at
    /// home, over HTTPS with the box's certificate.
    #[serde(default = "laptop_tcp")]
    pub tcp_port: u16,
    /// UDP: WebRTC, from anywhere, like iroh's port.
    #[serde(default = "laptop_udp")]
    pub udp_port: u16,
    /// The web server laptops' pages come from.
    #[serde(default = "laptop_target")]
    pub target: String,
    /// Plain HTTP on the home network instead of HTTPS: development only,
    /// for a box with no certificate.
    #[serde(default)]
    pub plain_http: bool,
    /// Box Desktop's address as laptops open it. Empty: `https://<the box's
    /// name>:7443`. Firefox's extension opens this address and keeps Box
    /// Desktop's service worker on it (docs/laptops.md, "Firefox").
    #[serde(default)]
    pub desktop_url: String,
}

impl Default for Laptops {
    fn default() -> Self {
        Self {
            tcp_port: laptop_tcp(),
            udp_port: laptop_udp(),
            target: laptop_target(),
            plain_http: false,
            desktop_url: String::new(),
        }
    }
}

fn laptop_tcp() -> u16 {
    7445
}
fn laptop_udp() -> u16 {
    7446
}
fn laptop_target() -> String {
    "box-desktop".into()
}

/// A certificate authority.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    /// A short name for the log and Box Desktop.
    pub name: String,
    /// Its ACME directory.
    pub directory: String,
    /// External account binding, where the authority wants one
    /// (Google Trust Services): key ID.
    #[serde(default)]
    pub eab_kid: Option<String>,
    /// And its key, base64url.
    #[serde(default)]
    pub eab_hmac_key: Option<String>,
}

fn data_dir() -> PathBuf {
    PathBuf::from("/data/zoomriya-connect")
}
fn certs_dir() -> PathBuf {
    PathBuf::from("/data/zoomriya-connect/certificate")
}
fn server() -> String {
    "https://connect.zoomriya.net".into()
}
fn port() -> u16 {
    443
}
fn api_socket() -> PathBuf {
    PathBuf::from("/run/zoomriya-connect/api.sock")
}
fn factory_token() -> PathBuf {
    PathBuf::from("/etc/zoomriya/factory-token")
}
fn targets() -> Vec<Target> {
    vec![Target {
        name: "gateway".into(),
        unix: Some(PathBuf::from("/run/zoomriya/gateway-link.sock")),
        tcp: None,
        proxy_protocol: true,
        tls: true,
    }]
}
fn authorities() -> Vec<Authority> {
    vec![Authority {
        name: "Let's Encrypt".into(),
        directory: "https://acme-v02.api.letsencrypt.org/directory".into(),
        eab_kid: None,
        eab_hmac_key: None,
    }]
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str("").unwrap_or_else(|_| unreachable!("every field has a default"))
    }
}

impl Config {
    /// Reads the file; a missing file means every default.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        for t in &cfg.targets {
            anyhow::ensure!(
                t.unix.is_some() != t.tcp.is_some(),
                "target {}: give either unix or tcp",
                t.name
            );
            if let Some(a) = t.tcp {
                anyhow::ensure!(
                    a.ip().is_loopback(),
                    "target {}: only on the box itself",
                    t.name
                );
            }
        }
        Ok(cfg)
    }

    /// The box key's file.
    pub fn box_key(&self) -> PathBuf {
        self.data_dir.join("box.key")
    }
    /// The extra certificate authority's certificates, if set.
    pub fn extra_roots(&self) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
        use rustls::pki_types::{CertificateDer, pem::PemObject};
        match &self.extra_ca_file {
            None => Ok(vec![]),
            Some(f) => CertificateDer::pem_file_iter(f)
                .with_context(|| format!("reading {}", f.display()))?
                .collect::<Result<Vec<_>, _>>()
                .with_context(|| format!("reading {}", f.display())),
        }
    }

    /// The DNS update key's file.
    pub fn dns_key(&self) -> PathBuf {
        self.data_dir.join("dns-update.key")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_example() {
        let d = Config::default();
        assert_eq!(d.port, 443);
        assert_eq!(d.targets[0].name, "gateway");
        let cfg: Config =
            toml::from_str(include_str!("../../deploy/connectd.toml")).expect("example");
        assert_eq!(cfg.targets.len(), 2);
    }
}
