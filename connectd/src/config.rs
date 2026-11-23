//! connectd's settings: `/etc/zoomriya-connect/connectd.toml`
//! (`deploy/connectd.toml` in this repository).

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Everything in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The box key and the device list. Never in backups
    /// (zoomriya-os docs/design.md §4).
    #[serde(default = "data_dir")]
    pub data_dir: PathBuf,
    /// The UDP port iroh listens on: 443, so a forwarded port is easy, and
    /// networks that allow only web traffic often let it through.
    #[serde(default = "port")]
    pub port: u16,
    /// Box Desktop's way in: a Unix socket only box-desktopd's group can use.
    #[serde(default = "api_socket")]
    pub api_socket: PathBuf,
    /// The box's own web servers that paired devices reach.
    #[serde(default = "targets")]
    pub targets: Vec<Target>,
    /// The box's own relay behind a Tor onion service, for devices with no
    /// direct path (docs/direct-plus.md §3). Off only in tests.
    #[serde(default = "yes")]
    pub tor: bool,
}

/// One of the box's web servers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// The name devices ask for: `apps`, `desktop`.
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
}

fn data_dir() -> PathBuf {
    PathBuf::from("/data/zoomriya-connect")
}
fn yes() -> bool {
    true
}
fn port() -> u16 {
    443
}
fn api_socket() -> PathBuf {
    PathBuf::from("/run/zoomriya-connect/api.sock")
}
/// The apps' gateway and Box Desktop, on their sockets for paired devices
/// (group zoomriya-link): plain HTTP, as the laptop app asks for them.
fn targets() -> Vec<Target> {
    ["apps", "desktop"]
        .into_iter()
        .map(|name| Target {
            name: name.into(),
            unix: Some(PathBuf::from(format!("/run/zoomriya-link/{name}.sock"))),
            tcp: None,
            proxy_protocol: false,
        })
        .collect()
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

    /// Tor's keys and state, the onion service's key among them.
    pub fn tor_state(&self) -> PathBuf {
        self.data_dir.join("tor")
    }

    /// What Tor downloads about the network.
    pub fn tor_cache(&self) -> PathBuf {
        self.data_dir.join("tor-cache")
    }

    /// The file holding the secret behind the box's DHT record.
    pub fn record_secret(&self) -> PathBuf {
        self.data_dir.join("record.key")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_example() {
        let d = Config::default();
        assert_eq!(d.port, 443);
        assert_eq!(d.targets[0].name, "apps");
        assert_eq!(d.targets[1].name, "desktop");
        let cfg: Config =
            toml::from_str(include_str!("../../deploy/connectd.toml")).expect("example");
        assert_eq!(cfg.targets.len(), 2);
    }
}
