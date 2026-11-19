//! What the browser extension (laptops) and connectd say to each other.
//!
//! Laptops can't run iroh, so they reach the box over WebRTC, which browsers
//! do themselves (docs/laptops.md). Setting it up takes two messages, sent to
//! connectd on the home network, or through the relay from away:
//!
//! | Step | From | What |
//! |---|---|---|
//! | Pairing | Laptop, at home only | [`LaptopPairRequest`]: the short code from Box Desktop, the laptop's device key and link key. Answered with [`LaptopPaired`], sealed by the box key |
//! | Connecting | Laptop | [`LaptopOffer`], signed by the device key, with the WebRTC offer. Answered with [`LaptopAnswer`], sealed by the box key: the extension checks it against the box key it pinned, which ties WebRTC's encryption to the real box |
//!
//! Away from home, the offer goes to a relay instead, which passes it to the
//! box and its answer back, as they are:
//!
//! | Path on the relay | Who | Proof |
//! |---|---|---|
//! | `POST` [`RELAY_CONNECT`] | The laptop: a [`LaptopOffer`], answered with `{"answer": …}` as at home | Its pass (`Authorization: Bearer`), and the request signed with the link key the pass names |
//! | `POST` [`RELAY_WAIT`] | The box, waiting for offers: answered with [`RelayedOffers`] when one comes, or empty after [`RELAY_WAIT_SECONDS`] | Signed with the box key |
//! | `POST` [`RELAY_ANSWER`] | The box: a [`RelayedAnswer`] | Signed with the box key |
//!
//! Then, on the data channel `zoomriya`, the extension sends the box's pages'
//! requests and connectd answers them from the box's web server:
//!
//! | Frame | Text (JSON) or binary | What |
//! |---|---|---|
//! | [`Frame::Request`] | Text | A request: method, path, headers. Its body follows as data frames |
//! | [`Frame::Response`] | Text | The answer's status and headers. Its body follows as data frames |
//! | Data | Binary: request ID (4 bytes, big-endian), `0`, then bytes | Part of a body |
//! | End | Binary: request ID, `1` | The body is complete |
//! | [`Frame::Cancel`] | Text | The page stopped listening (a live event stream closed) |
//! | [`Frame::Error`] | Text | The box couldn't answer |

use serde::{Deserialize, Serialize};
use zoomriya_keys::Key;

/// The data channel's label.
pub const CHANNEL: &str = "zoomriya";
/// Biggest part of a body in one data frame.
pub const MAX_CHUNK: usize = 16 * 1024;
/// How far a laptop's clock may be from the box's, in seconds.
pub const OFFER_CLOCK: u64 = 300;
/// A pairing code: tries before it's used up.
pub const CODE_TRIES: u32 = 3;
/// On the relay: a laptop's offer, from away.
pub const RELAY_CONNECT: &str = "/zoomriya/v1/laptop/connect";
/// On the relay: the box waiting for laptops' offers.
pub const RELAY_WAIT: &str = "/zoomriya/v1/laptop/wait";
/// On the relay: the box's answer to an offer.
pub const RELAY_ANSWER: &str = "/zoomriya/v1/laptop/answer";
/// How long the relay holds the box's wait before answering it empty.
pub const RELAY_WAIT_SECONDS: u64 = 25;

/// Pairing a laptop, at home.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaptopPairRequest {
    /// The short code Box Desktop shows, such as `4K7P-2QXM`.
    pub code: String,
    /// The laptop's device key: P-256, SEC1, base64url. Made by the browser,
    /// which won't export it.
    pub device_key: String,
    /// The laptop's link key (Ed25519), for passes.
    pub link: Key,
    /// The device key's signature over the link key, raw, base64url.
    pub link_sig: String,
    /// What the laptop calls itself, such as "Anna's laptop".
    pub name: String,
}

/// What a laptop gets from pairing, sealed by the box key
/// ([`zoomriya_keys::Purpose::LaptopPaired`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaptopPaired {
    /// The box key, which the extension pins.
    pub box_key: Key,
    /// The box's name, such as `maplehouse.zoomriya.app`.
    pub fqdn: String,
    /// The laptop's ID on the box.
    pub device: String,
    /// Its first pass, for the relay.
    pub pass: String,
    /// When the pass runs out.
    pub until: u64,
    /// The ZOOMRIYA Connect server the box uses.
    pub server: String,
    /// The relays, for connecting from away.
    #[serde(default)]
    pub relays: Vec<String>,
    /// STUN servers, so WebRTC learns the laptop's public address.
    #[serde(default)]
    pub ice: Vec<IceServer>,
    /// Box Desktop's address, such as `https://maplehouse.zoomriya.app:7443`.
    /// Firefox opens the box here (docs/laptops.md, "Firefox").
    #[serde(default)]
    pub desktop: String,
    /// The keys of the box's certificate: SHA-256 of its SubjectPublicKeyInfo,
    /// base64. Firefox checks every page of Box Desktop's address against them.
    #[serde(default)]
    pub tls_keys: Vec<String>,
}

/// A laptop asking to connect. `sig` is the device key's signature over
/// [`LaptopOffer::signed_bytes`] for [`zoomriya_keys::Purpose::LaptopOffer`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaptopOffer {
    /// The laptop's link key: who it is.
    pub link: Key,
    /// When it made the offer, in seconds since 1970.
    pub time: u64,
    /// The WebRTC offer.
    pub sdp: String,
    /// The signature, raw, base64url.
    pub sig: String,
}

impl LaptopOffer {
    /// What the device key signs: the link key, the time and the offer.
    pub fn signed_bytes(&self) -> Vec<u8> {
        format!("{}\n{}\n{}", self.link, self.time, self.sdp).into_bytes()
    }
}

/// The box's answer, sealed by the box key
/// ([`zoomriya_keys::Purpose::LaptopAnswer`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaptopAnswer {
    /// The offer it answers: SHA-256 of its SDP, base64url.
    pub offer: String,
    /// The WebRTC answer.
    pub sdp: String,
    /// A fresh pass, so a laptop in use never runs out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<String>,
    /// When it runs out.
    #[serde(default)]
    pub until: u64,
    /// The relays now.
    #[serde(default)]
    pub relays: Vec<String>,
    /// STUN servers now.
    #[serde(default)]
    pub ice: Vec<IceServer>,
    /// The keys of the box's certificate now, as in [`LaptopPaired::tls_keys`].
    #[serde(default)]
    pub tls_keys: Vec<String>,
}

/// One offer from away, as the relay hands it to the box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayedOffer {
    /// The relay's number for it.
    pub id: u64,
    /// The offer, as the laptop sent it.
    pub offer: LaptopOffer,
}

/// What the box's wait gets: offers, possibly none.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelayedOffers {
    /// The offers waiting.
    pub offers: Vec<RelayedOffer>,
}

/// The box's answer to a relayed offer: the sealed answer, or why not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayedAnswer {
    /// The relay's number for the offer.
    pub id: u64,
    /// The sealed [`LaptopAnswer`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Why there's no answer, for the laptop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::ErrorBody>,
}

/// The ICE servers a laptop may use: the guests' relay (TURN), with
/// one-hour credentials the box got from its server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaptopIce {
    /// As WebRTC wants them: `{urls, username, credential}`.
    pub servers: Vec<IceServer>,
}

/// One ICE server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IceServer {
    /// Such as `turn:turn.zoomriya.net:3478?transport=udp`.
    pub urls: Vec<String>,
    /// The credential's username.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// The credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

/// A text frame on the data channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    /// A request from one of the box's pages.
    Request {
        /// Its ID, unique on this connection.
        id: u32,
        /// `GET`, `POST` and so on.
        method: String,
        /// The path and query, such as `/api/events`.
        path: String,
        /// Headers, lowercase names.
        headers: Vec<(String, String)>,
    },
    /// The answer's status and headers.
    Response {
        /// The request's ID.
        id: u32,
        /// The HTTP status.
        status: u16,
        /// Headers, lowercase names.
        headers: Vec<(String, String)>,
    },
    /// The page stopped listening.
    Cancel {
        /// The request's ID.
        id: u32,
    },
    /// The box couldn't answer.
    Error {
        /// The request's ID.
        id: u32,
        /// Plain words.
        message: String,
    },
}

/// A binary frame: part of a body, or its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data<'a> {
    /// Part of the body of request `id`'s request or answer.
    Chunk(u32, &'a [u8]),
    /// The body is complete.
    End(u32),
}

impl Data<'_> {
    /// The frame's bytes.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Data::Chunk(id, bytes) => {
                let mut out = Vec::with_capacity(5 + bytes.len());
                out.extend_from_slice(&id.to_be_bytes());
                out.push(0);
                out.extend_from_slice(bytes);
                out
            }
            Data::End(id) => {
                let mut out = id.to_be_bytes().to_vec();
                out.push(1);
                out
            }
        }
    }

    /// Reads a binary frame.
    pub fn decode(bytes: &[u8]) -> Option<Data<'_>> {
        let id = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?);
        match bytes.get(4)? {
            0 => Some(Data::Chunk(id, &bytes[5..])),
            1 if bytes.len() == 5 => Some(Data::End(id)),
            _ => None,
        }
    }
}

/// A pairing code for a laptop: 8 letters and digits, shown as `4K7P-2QXM`,
/// without the ones that look alike.
pub fn new_code(random: [u8; 8]) -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    let c: String = random
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect();
    format!("{}-{}", &c[..4], &c[4..])
}

/// A typed code as compared: capitals, without the dash or spaces.
pub fn normalise_code(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_frames_round_trip() {
        let chunk = Data::Chunk(7, b"hello");
        assert_eq!(Data::decode(&chunk.encode()), Some(chunk));
        assert_eq!(Data::decode(&Data::End(7).encode()), Some(Data::End(7)));
        assert_eq!(Data::decode(&[0, 0, 0, 7, 9]), None);
        assert_eq!(Data::decode(&[0, 0]), None);
    }

    #[test]
    fn text_frames_read_as_documented() {
        let f: Frame = serde_json::from_str(
            r#"{"t":"request","id":1,"method":"GET","path":"/api/events","headers":[["accept","text/event-stream"]]}"#,
        )
        .expect("reads");
        assert!(matches!(f, Frame::Request { id: 1, .. }));
        let f: Frame = serde_json::from_str(r#"{"t":"cancel","id":1}"#).expect("reads");
        assert_eq!(f, Frame::Cancel { id: 1 });
    }

    #[test]
    fn codes() {
        let c = new_code([0, 1, 2, 3, 250, 251, 252, 253]);
        assert_eq!(c.len(), 9);
        assert_eq!(&c[4..5], "-");
        assert!(!c.contains(['0', '1', 'I', 'L', 'O']));
        assert_eq!(normalise_code(" 4k7p-2qxm "), "4K7P2QXM");
        assert_eq!(normalise_code(&c), c.replace('-', ""));
    }
}
