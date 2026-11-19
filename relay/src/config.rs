//! The relay's settings, from a TOML file (`deploy/relay.toml` is an example).

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use zoomriya_serve::CertSource;

/// Everything in the file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where the relay keeps its list of boxes between restarts.
    pub state_dir: PathBuf,
    /// HTTPS, where the relay itself and our API live.
    pub https: Https,
    /// Plain HTTP, for the captive-portal check only.
    #[serde(default)]
    pub http: Option<Listen>,
    /// QUIC address discovery: tells each side its public address.
    #[serde(default)]
    pub quic: Option<Listen>,
    /// Numbers for our dashboard. Keep it off the internet.
    #[serde(default)]
    pub metrics: Option<Listen>,
    /// Which boxes may use the relay.
    pub boxes: Boxes,
    /// Limits.
    #[serde(default)]
    pub limits: Limits,
    /// The GUI and its admin API.
    #[serde(default)]
    pub admin: Option<zoomriya_serve::admin::AdminConfig>,
}

/// The HTTPS listener.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Https {
    /// Usually `[::]:443`.
    pub bind: SocketAddr,
    /// Where the certificate comes from.
    pub cert: CertSource,
}

/// A plain listener.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listen {
    /// The address and port.
    pub bind: SocketAddr,
}

/// Where the list of boxes comes from.
#[derive(Debug, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum Boxes {
    /// ZOOMRIYA's relays, and a family's full server: the control plane says.
    ControlPlane {
        /// Such as `https://connect.zoomriya.net`.
        url: String,
        /// A file holding the control plane to relay key.
        key_file: PathBuf,
        /// How often to ask, in seconds. Each time, the relay also reports
        /// each box's use, for the monthly allowance.
        #[serde(default = "ten")]
        poll_seconds: u64,
        /// This relay's name in the GUI, such as `relay1.zoomriya.net`.
        name: String,
    },
    /// A family's own relay on its own: each box introduces itself with a
    /// one-time code (`zoomriya-relay enrol-code`).
    Enrolled,
}

fn ten() -> u64 {
    10
}

/// Limits.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Clock difference allowed on passes and lists, in seconds.
    #[serde(default = "ten_minutes")]
    pub clock_allowance_seconds: u64,
    /// Longest pass accepted, in days.
    #[serde(default = "thirty_one")]
    pub longest_pass_days: u64,
    /// Fastest each connection may send to the relay, in bytes a second.
    /// No limit if left out (a family's own relay). The monthly allowance is
    /// set in the control plane's GUI.
    #[serde(default)]
    pub client_bytes_per_second: Option<u32>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            clock_allowance_seconds: ten_minutes(),
            longest_pass_days: thirty_one(),
            client_bytes_per_second: None,
        }
    }
}

fn ten_minutes() -> u64 {
    600
}

fn thirty_one() -> u64 {
    31
}

impl Config {
    /// Reads and checks the file.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        anyhow::ensure!(
            cfg.limits.clock_allowance_seconds <= 3600,
            "clock_allowance_seconds: at most an hour"
        );
        anyhow::ensure!(
            (1..=366).contains(&cfg.limits.longest_pass_days),
            "longest_pass_days: 1 to 366"
        );
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_reads() {
        let text = include_str!("../../deploy/relay.toml");
        let cfg: Config = toml::from_str(text).expect("example reads");
        assert!(matches!(cfg.boxes, Boxes::ControlPlane { .. }));
        let text = include_str!("../../deploy/family-relay.toml");
        let cfg: Config = toml::from_str(text).expect("family example reads");
        assert!(matches!(cfg.boxes, Boxes::Enrolled));
    }
}
