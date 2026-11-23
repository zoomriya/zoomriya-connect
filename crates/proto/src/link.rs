//! What the phone and laptop apps' link and connectd say to each other over
//! iroh.
//!
//! | ALPN | What | When |
//! |---|---|---|
//! | `zoomriya/pair/1` | Pairing: a phone shows the QR code's secret and its keys; a laptop proves it knows the short code ([`crate::laptop`]). The box adds it to its list | At home only, while Box Desktop shows the code |
//! | `zoomriya/link/1` | Everything else | Any time, from any path |
//!
//! On `zoomriya/link/1` every stream the device opens starts with one byte
//! saying what it is:
//!
//! | Byte | Stream | Then |
//! |---|---|---|
//! | `1` | Control | JSON lines: [`ToBox`] from the device, [`FromBox`] from the box |
//! | `2` | Web | One byte, the target's name's length, the name (`apps`, `desktop`), then the request as the box's web server takes it: plain HTTP for `apps` and `desktop` |

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use zoomriya_keys::Key;

/// The family connection.
pub const LINK_ALPN: &[u8] = b"zoomriya/link/1";
/// Pairing.
pub const PAIR_ALPN: &[u8] = b"zoomriya/pair/1";

/// How the box announces its link on the home network (mDNS, DNS-SD): UDP,
/// at the box's port, with its key in the TXT record [`MDNS_KEY`].
pub const MDNS_SERVICE: &str = "_zoomriya-link._udp.local.";
/// The TXT record's key holding the box key, z-base-32.
pub const MDNS_KEY: &str = "key";

/// A control stream.
pub const STREAM_CONTROL: u8 = 1;
/// A web stream to one of the box's own web servers.
pub const STREAM_WEB: u8 = 2;

/// Longest JSON line on a control stream.
pub const MAX_LINE: usize = 64 * 1024;

/// The device's first message, and later ones, on the control stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToBox {
    /// Hello: which protocol version.
    Hello {
        /// The link's protocol version.
        version: u32,
    },
    /// What iroh measured on the last connections, for Box Desktop.
    Report(ConnectionReport),
}

/// The box's answers on the control stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromBox {
    /// The box let the device in.
    Welcome {
        /// Where the device can reach the box directly next time: home, IPv6,
        /// a forwarded port.
        addrs: Vec<SocketAddr>,
    },
    /// The box's addresses changed.
    Addrs {
        /// The new list.
        addrs: Vec<SocketAddr>,
    },
    /// Something was refused. The connection stays up.
    Refused {
        /// A stable code.
        code: String,
        /// Plain words.
        message: String,
    },
}

/// The pairing request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    /// The one-time secret from the QR code.
    pub secret: String,
    /// The device key: P-256, SEC1 (compressed or not), base64url. Made in the
    /// security chip and never leaving it.
    pub device_key: String,
    /// The link key (the device's iroh endpoint key), which this connection
    /// comes from.
    pub link: Key,
    /// The device key's signature over the link key
    /// ([`zoomriya_keys::link_key_message`]), DER or raw, base64url.
    pub link_sig: String,
    /// What the device calls itself, such as "Mum's phone".
    pub name: String,
}

/// The box's answer to pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PairResponse {
    /// Paired.
    #[serde(rename = "paired")]
    Paired {
        /// Where to reach the box directly.
        addrs: Vec<SocketAddr>,
        /// The secret behind the box's record in the DHT, base64url
        /// ([`zoomriya_keys::record`]): how the device finds the box from
        /// away.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<String>,
        /// The box's onion address, `….onion`: its own relay through Tor,
        /// for when no direct path works.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        onion: Option<String>,
    },
    /// A laptop's pairing, step 2: the box's SPAKE2 message and its
    /// confirmation ([`crate::laptop`]).
    #[serde(rename = "challenge")]
    Challenge {
        /// The box's SPAKE2 message, base64url.
        spake: String,
        /// The box's confirmation over the transcript, base64url.
        confirm: String,
    },
    /// Refused.
    #[serde(rename = "refused")]
    Refused {
        /// A stable code.
        code: String,
        /// Plain words.
        message: String,
    },
}

/// What the QR code on the box's screen holds, as a URL:
/// `zoomriya://pair?box=<key>&s=<secret>&a=<addr>&a=<addr>`: the box key
/// and its home addresses are all a device needs to pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode {
    /// The box key, which the app pins.
    pub box_key: Key,
    /// The one-time secret.
    pub secret: String,
    /// The box's addresses on the home network.
    pub addrs: Vec<SocketAddr>,
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn dec(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

impl PairingCode {
    /// The URL the QR code shows.
    pub fn to_url(&self) -> String {
        let mut u = format!(
            "zoomriya://pair?box={}&s={}",
            self.box_key,
            enc(&self.secret)
        );
        for a in &self.addrs {
            u.push_str(&format!("&a={}", enc(&a.to_string())));
        }
        u
    }

    /// Reads a QR code's URL.
    pub fn from_url(url: &str) -> Option<Self> {
        let query = url.strip_prefix("zoomriya://pair?")?;
        let (mut box_key, mut secret) = (None, None);
        let mut addrs = Vec::new();
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=')?;
            let v = dec(v)?;
            match k {
                "box" => box_key = Some(zoomriya_keys::key_from_str(&v).ok()?),
                "s" => secret = Some(v),
                "a" => addrs.push(v.parse().ok()?),
                _ => {}
            }
        }
        Some(Self {
            box_key: Key(box_key?),
            secret: secret?,
            addrs,
        })
    }
}

/// What iroh measured on one connection, sent to the box afterwards
/// (docs/architecture.md, "Measuring connections"). Kept on the box only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConnectionReport {
    /// When the connection started, in seconds since 1970.
    pub started: u64,
    /// Milliseconds until the first path worked.
    #[serde(default)]
    pub ms_to_connect: Option<u64>,
    /// Milliseconds until a direct path was chosen; none if it never was.
    #[serde(default)]
    pub ms_to_direct: Option<u64>,
    /// Round-trip time on the box's own relay (over Tor), in milliseconds.
    #[serde(default)]
    pub rtt_relay_ms: Option<u64>,
    /// Round-trip time direct, in milliseconds.
    #[serde(default)]
    pub rtt_direct_ms: Option<u64>,
    /// Seconds spent on the relay.
    #[serde(default)]
    pub secs_on_relay: u64,
    /// Path switches.
    #[serde(default)]
    pub path_switches: u32,
    /// Bytes sent and received directly.
    #[serde(default)]
    pub bytes_direct: u64,
    /// Bytes sent and received through the relay.
    #[serde(default)]
    pub bytes_relay: u64,
    /// Hole punching tried, and whether it worked.
    #[serde(default)]
    pub holepunch: Option<bool>,
    /// Why it couldn't connect or dropped: `no_internet`, `relay_unreachable`,
    /// `box_offline`, `not_paired`, `timed_out`, `network_change`,
    /// `relay_restart`, `box_restart`, `lost_signal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// The kind of network the device was on: `wifi`, `mobile`, `wired`, `other`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_codes_round_trip() {
        let code = PairingCode {
            box_key: Key(zoomriya_keys::SecretKey::generate().public()),
            secret: "Ab3-x_9&z".into(),
            addrs: vec![
                "192.168.1.20:443".parse().expect("addr"),
                "[fd00::20]:443".parse().expect("addr"),
            ],
        };
        let url = code.to_url();
        assert!(url.starts_with("zoomriya://pair?box="));
        assert_eq!(PairingCode::from_url(&url), Some(code));
        assert_eq!(PairingCode::from_url("https://example.com"), None);
        assert_eq!(PairingCode::from_url("zoomriya://pair?box=x"), None);
    }

    #[test]
    fn messages_read_as_documented() {
        let m: ToBox = serde_json::from_str(r#"{"type":"hello","version":1}"#).expect("reads");
        assert_eq!(m, ToBox::Hello { version: 1 });
        let r: ToBox = serde_json::from_str(r#"{"type":"report","started":1}"#).expect("reads");
        assert!(matches!(
            r,
            ToBox::Report(ConnectionReport { started: 1, .. })
        ));
    }
}
