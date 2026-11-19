//! The control plane's settings, from a TOML file (`deploy/control-plane.toml`
//! is an example).

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use zoomriya_serve::CertSource;

/// Everything in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The SQLite database: registrations and what goes with them.
    pub database: PathBuf,
    /// Names and addresses, for `/.well-known/zoomriya-connect.json` and DNS.
    pub names: Names,
    /// The API, over HTTPS.
    pub https: Https,
    /// Authoritative DNS for box names.
    pub dns: Dns,
    /// The relays.
    pub relays: Relays,
    /// Rules for registering boxes.
    #[serde(default)]
    pub registration: Registration,
    /// Guests: their relay (TURN) and setup messages.
    #[serde(default)]
    pub guests: Option<Guests>,
    /// The second DNS server's copy of the zone.
    #[serde(default)]
    pub zone_copy: Option<ZoneCopyKeys>,
    /// The GUI and its admin API.
    #[serde(default)]
    pub admin: Option<zoomriya_serve::admin::AdminConfig>,
    /// ZOOMRIYA Connect accounts, issued by operators.
    #[serde(default)]
    pub accounts: Accounts,
}

/// ZOOMRIYA Connect accounts.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounts {
    /// Every box needs an account to register.
    #[serde(default = "yes")]
    pub required: bool,
    /// The families' account page's built files, served at `/account/`.
    #[serde(default = "account_page")]
    pub page_dir: PathBuf,
}

impl Default for Accounts {
    fn default() -> Self {
        Self {
            required: true,
            page_dir: account_page(),
        }
    }
}

fn yes() -> bool {
    true
}

fn account_page() -> PathBuf {
    PathBuf::from("/usr/share/zoomriya-connect/account")
}

/// Guests' relay (TURN).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guests {
    /// The guests' relay, as WebRTC wants it, such as
    /// `turn:turn.zoomriya.net:3478?transport=udp`.
    pub turn_urls: Vec<String>,
    /// The shared secrets, one a line, the newest first. Credentials are made
    /// with the first; the guests' relay accepts all during a change.
    pub turn_secrets_file: PathBuf,
}

/// Who may copy the zone: the second DNS server.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneCopyKeys {
    /// A file of keys, one a line.
    pub keys_file: PathBuf,
}

/// Our names.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Names {
    /// Where box names live, such as `zoomriya.app`.
    pub zone: String,
    /// Where address records are answered in DNS, such as `dns.zoomriya.net`
    /// (`_iroh.<key>.dns.zoomriya.net`). Its NS records must point here.
    pub address_zone: String,
    /// This server's API, such as `https://connect.zoomriya.net`.
    pub control_plane_url: String,
    /// The guest page, such as `https://share.zoomriya.net`.
    pub share_url: String,
    /// Name servers for the zones, such as `ns1.zoomriya.net`. The first is the SOA's.
    pub nameservers: Vec<String>,
    /// The SOA's contact, written as a name: `hostmaster.zoomriya.net`.
    pub hostmaster: String,
    /// Certificate authorities allowed to issue for box names (CAA), such as
    /// `letsencrypt.org` and `pki.goog`. Empty: no CAA records.
    #[serde(default)]
    pub certificate_authorities: Vec<String>,
}

/// The HTTPS listener.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Https {
    /// Usually `[::]:443`.
    pub bind: SocketAddr,
    /// Where the certificate comes from.
    pub cert: CertSource,
}

/// The DNS server.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dns {
    /// Usually `[::]:53`, UDP and TCP.
    pub bind: SocketAddr,
    /// Answers a second for each /24 (IPv4) or /56 (IPv6). Beyond it, answers
    /// are cut short so the asker must come back over TCP.
    #[serde(default = "dns_rate")]
    pub answers_per_second: u32,
}

fn dns_rate() -> u32 {
    50
}

/// The relays.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relays {
    /// Their URLs, in order of preference, such as `https://relay1.zoomriya.net`.
    pub urls: Vec<String>,
    /// A file of control plane to relay keys, one a line. During a change,
    /// old and new both work.
    pub keys_file: PathBuf,
}

/// Rules for registering boxes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    /// New boxes a day from one internet address (an IPv6 /64). Boxes with a
    /// factory token don't count.
    #[serde(default = "ten")]
    pub per_address_per_day: u32,
    /// Days a released name waits before anyone else can take it.
    #[serde(default = "ninety")]
    pub released_name_days: u32,
    /// A file of extra words refused in names, one a line: brand names and
    /// anything else that turns up. Read again on restart.
    #[serde(default)]
    pub blocked_words_file: Option<PathBuf>,
    /// Clock difference allowed on signed requests, in seconds.
    #[serde(default = "ten_minutes")]
    pub clock_allowance_seconds: u64,
}

impl Default for Registration {
    fn default() -> Self {
        Self {
            per_address_per_day: ten(),
            released_name_days: ninety(),
            blocked_words_file: None,
            clock_allowance_seconds: ten_minutes(),
        }
    }
}

fn ten() -> u32 {
    10
}
fn ninety() -> u32 {
    90
}
fn ten_minutes() -> u64 {
    600
}

impl Config {
    /// Reads and checks the file.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        cfg.check()?;
        Ok(cfg)
    }

    fn check(&self) -> Result<()> {
        anyhow::ensure!(
            !self.names.nameservers.is_empty(),
            "names.nameservers is empty"
        );
        anyhow::ensure!(!self.relays.urls.is_empty(), "relays.urls is empty");
        anyhow::ensure!(
            self.registration.clock_allowance_seconds <= 3600,
            "clock_allowance_seconds: at most an hour"
        );
        Ok(())
    }

    /// The extra blocked words, if a file is set.
    pub fn blocked_words(&self) -> Result<String> {
        match &self.registration.blocked_words_file {
            None => Ok(String::new()),
            Some(p) => {
                std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))
            }
        }
    }

    /// The control plane to relay keys.
    pub fn relay_keys(&self) -> Result<Vec<String>> {
        read_keys(&self.relays.keys_file)
    }
}

/// Reads a file of secrets, one a line, each at least 32 characters.
pub fn read_keys(p: &std::path::Path) -> Result<Vec<String>> {
    let text = {
        let text =
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        let keys: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect();
        keys
    };
    anyhow::ensure!(!text.is_empty(), "no keys in {}", p.display());
    anyhow::ensure!(
        text.iter().all(|k| k.len() >= 32),
        "keys in {} must be at least 32 characters",
        p.display()
    );
    Ok(text)
}

/// The second DNS server's settings: DNS only, copying the first.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecondaryConfig {
    /// The same names as the first server.
    pub names: Names,
    /// DNS.
    pub dns: Dns,
    /// The first server's API, such as `https://connect.zoomriya.net`.
    pub primary_url: String,
    /// A file holding one of the first server's zone-copy keys.
    pub key_file: PathBuf,
    /// How often to ask, in seconds.
    #[serde(default = "five")]
    pub poll_seconds: u64,
}

fn five() -> u64 {
    5
}

impl SecondaryConfig {
    /// Reads the file.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_reads() {
        let cfg: Config =
            toml::from_str(include_str!("../../deploy/control-plane.toml")).expect("reads");
        cfg.check().expect("checks");
        assert_eq!(cfg.names.zone, "zoomriya.app");
        assert_eq!(cfg.registration.per_address_per_day, 10);
        assert!(cfg.guests.is_some());
        let second: SecondaryConfig =
            toml::from_str(include_str!("../../deploy/control-plane-ns2.toml")).expect("reads");
        assert_eq!(second.names.zone, "zoomriya.app");
    }
}
