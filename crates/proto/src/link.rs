//! What the phone app's link (or the laptop extension) and connectd say to
//! each other over iroh.
//!
//! | ALPN | What | When |
//! |---|---|---|
//! | `zoomriya/pair/1` | Pairing: the device shows the QR code's secret and its keys; the box gives it a first pass | At home only, while Box Desktop shows the code |
//! | `zoomriya/link/1` | Everything else | Any time, from any path |
//!
//! On `zoomriya/link/1` every stream the device opens starts with one byte
//! saying what it is:
//!
//! | Byte | Stream | Then |
//! |---|---|---|
//! | `1` | Control | JSON lines: [`ToBox`] from the device, [`FromBox`] from the box |
//! | `2` | Web | One byte, the target's name's length, the name (`gateway`, `box-desktop`), then the browser's own TLS, passed through unopened |

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use zoomriya_keys::Key;

/// The family connection.
pub const LINK_ALPN: &[u8] = b"zoomriya/link/1";
/// Pairing.
pub const PAIR_ALPN: &[u8] = b"zoomriya/pair/1";

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
    /// Hello: which protocol version, and how long its pass has left.
    Hello {
        /// The link's protocol version.
        version: u32,
        /// When its pass runs out, in seconds since 1970.
        pass_until: u64,
    },
    /// A new link key, signed by the device key in the security chip, for the
    /// next pass. The old link key stops working once the box answers.
    Renew {
        /// The new link key: the device's next iroh endpoint ID.
        link: Key,
        /// The device key's ECDSA P-256 signature over
        /// [`zoomriya_keys::link_key_message`], DER or raw, base64url.
        sig: String,
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
        /// The box's name, such as `maplehouse.zoomriya.app`.
        fqdn: String,
        /// Where the device can reach the box directly next time: home, IPv6,
        /// the door, a forwarded port.
        addrs: Vec<SocketAddr>,
        /// `true` when the pass has under three weeks left: the device should
        /// send [`ToBox::Renew`].
        renew: bool,
    },
    /// A new pass, after [`ToBox::Renew`].
    Pass {
        /// The pass token.
        pass: String,
        /// When it runs out.
        until: u64,
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
    /// The first link key, which this connection comes from.
    pub link: Key,
    /// The device key's signature over the link key, as in [`ToBox::Renew`].
    pub link_sig: String,
    /// What the device calls itself, such as "Mum's phone".
    pub name: String,
    /// For an admin's phone: the Ed25519 key its admin notes name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_key: Option<Key>,
}

/// The box's answer to pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PairResponse {
    /// Paired.
    #[serde(rename = "paired")]
    Paired {
        /// The device's first pass.
        pass: String,
        /// When it runs out.
        until: u64,
        /// The box's name.
        fqdn: String,
        /// Where to reach the box directly.
        addrs: Vec<SocketAddr>,
        /// For an admin's phone: its admin note, sealed by the box.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        admin_note: Option<String>,
        /// The ZOOMRIYA Connect server the box uses, for the app's settings.
        server: String,
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
/// `zoomriya://pair?box=<key>&s=<secret>&n=<fqdn>&a=<addr>&a=<addr>&srv=<url>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode {
    /// The box key, which the app pins.
    pub box_key: Key,
    /// The one-time secret.
    pub secret: String,
    /// The box's name.
    pub fqdn: String,
    /// The box's addresses on the home network.
    pub addrs: Vec<SocketAddr>,
    /// The ZOOMRIYA Connect server, such as `https://connect.zoomriya.net`.
    pub server: String,
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
            "zoomriya://pair?box={}&s={}&n={}",
            self.box_key,
            enc(&self.secret),
            enc(&self.fqdn)
        );
        for a in &self.addrs {
            u.push_str(&format!("&a={}", enc(&a.to_string())));
        }
        u.push_str(&format!("&srv={}", enc(&self.server)));
        u
    }

    /// Reads a QR code's URL.
    pub fn from_url(url: &str) -> Option<Self> {
        let query = url.strip_prefix("zoomriya://pair?")?;
        let (mut box_key, mut secret, mut fqdn, mut server) = (None, None, None, None);
        let mut addrs = Vec::new();
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=')?;
            let v = dec(v)?;
            match k {
                "box" => box_key = Some(zoomriya_keys::key_from_str(&v).ok()?),
                "s" => secret = Some(v),
                "n" => fqdn = Some(v),
                "a" => addrs.push(v.parse().ok()?),
                "srv" => server = Some(v),
                _ => {}
            }
        }
        Some(Self {
            box_key: Key(box_key?),
            secret: secret?,
            fqdn: fqdn?,
            addrs,
            server: server?,
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
    /// Round-trip time on the relay, in milliseconds.
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
    /// `box_offline`, `pass_refused`, `timed_out`, `network_change`,
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
            fqdn: "maplehouse.zoomriya.app".into(),
            addrs: vec![
                "192.168.1.20:443".parse().expect("addr"),
                "[fd00::20]:443".parse().expect("addr"),
            ],
            server: "https://connect.zoomriya.net".into(),
        };
        let url = code.to_url();
        assert!(url.starts_with("zoomriya://pair?box="));
        assert_eq!(PairingCode::from_url(&url), Some(code));
        assert_eq!(PairingCode::from_url("https://example.com"), None);
        assert_eq!(PairingCode::from_url("zoomriya://pair?box=x"), None);
    }

    #[test]
    fn messages_read_as_documented() {
        let m: ToBox =
            serde_json::from_str(r#"{"type":"hello","version":1,"pass_until":5}"#).expect("reads");
        assert_eq!(
            m,
            ToBox::Hello {
                version: 1,
                pass_until: 5
            }
        );
        let r: ToBox = serde_json::from_str(r#"{"type":"report","started":1}"#).expect("reads");
        assert!(matches!(
            r,
            ToBox::Report(ConnectionReport { started: 1, .. })
        ));
    }
}
