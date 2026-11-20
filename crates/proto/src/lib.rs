//! Messages shared by the link, connectd, the relay and the control plane.
//!
//! Every request carries a protocol version. Servers accept the current
//! version and the one before; older ones get [`ErrorBody`] with code
//! `please_update` (docs/architecture.md, "Versions and updates").

use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};
use zoomriya_keys::Key;

pub mod addrs;
pub mod laptop;
pub mod link;
pub mod names;
pub mod stun;

/// The protocol version this build speaks.
pub const PROTOCOL_VERSION: u32 = 1;
/// The oldest version servers still accept.
pub const MIN_PROTOCOL_VERSION: u32 = 1;

/// The header carrying the protocol version on every API request.
pub const VERSION_HEADER: &str = "zoomriya-connect-version";

/// What a server says when it refuses a request. `code` is stable, for
/// programs; `message` is plain words, for Box Desktop to show as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// A stable code, such as `name_taken`.
    pub code: String,
    /// What to show the family.
    pub message: String,
}

/// The box's addresses on its home network, for its name. Private addresses
/// only (see [`addrs`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeAddrs {
    /// IPv4 addresses on the home network.
    #[serde(default)]
    pub ipv4: Vec<Ipv4Addr>,
    /// IPv6 unique local addresses on the home network.
    #[serde(default)]
    pub ipv6: Vec<Ipv6Addr>,
}

/// `POST /v1/boxes`: a box asks for a name. Signed with the box key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// The name the family picked, such as `maplehouse`.
    pub name: String,
    /// The box key: its iroh endpoint ID.
    pub box_key: Key,
    /// The public half of the box's DNS update key.
    pub dns_update_key: Key,
    /// Where the name points: the box on the home network.
    #[serde(default)]
    pub home: HomeAddrs,
    /// The factory token of a ready-made box, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_token: Option<String>,
    /// The family's ZOOMRIYA Connect account, the first time: links the box to
    /// it. Never kept on the box; registering again doesn't need it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountLogin>,
}

/// A ZOOMRIYA Connect account's sign-in. Separate from ZOOMRIYA ID: an
/// operator issues it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountLogin {
    /// The username the operator gave.
    pub username: String,
    /// The password.
    pub password: String,
}

impl std::fmt::Debug for AccountLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AccountLogin({})", self.username)
    }
}

/// The answer to a registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterResponse {
    /// The name, as registered.
    pub name: String,
    /// The box's full name, such as `maplehouse.zoomriya.app`.
    pub fqdn: String,
    /// The guests' door, such as `maplehouse-share.zoomriya.app`.
    pub share_fqdn: String,
    /// Whether the box registered with a factory token.
    pub factory: bool,
    /// The account the box is linked to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// `PUT /v1/boxes/{name}/home`: where the name points. Signed with the DNS update key.
pub type HomeRequest = HomeAddrs;

/// Which name a certificate check is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcmeTarget {
    /// `_acme-challenge.<name>.zoomriya.app`
    Box,
    /// `_acme-challenge.<name>-share.zoomriya.app`
    Door,
}

/// `PUT /v1/boxes/{name}/acme`: the temporary records for a certificate check.
/// Signed with the DNS update key. An empty list removes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcmeRequest {
    /// Which name.
    pub target: AcmeTarget,
    /// The TXT values the certificate authority asked for. At most four.
    pub values: Vec<String>,
}

/// `PUT /v1/boxes/{name}/door`: the guests' door is open at these public
/// addresses. Signed with the DNS update key. Each address must be the one
/// the request came from (IPv4) or in the same /56 (IPv6), so a box can only
/// point its door at its own home.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoorRequest {
    /// The router's public IPv4 address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<Ipv4Addr>,
    /// The door's own public IPv6 address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<Ipv6Addr>,
}

/// A box's standing, as relays need to know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxState {
    /// Registered. Its devices may use the relay.
    Active,
    /// Registered with a factory token: higher relay limits.
    ActiveFactory,
    /// An admin marked it stolen. Refused, with its devices.
    Stolen,
    /// No longer registered.
    Gone,
}

/// One change in the list of registered boxes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxChange {
    /// The box key.
    pub key: Key,
    /// Its standing now.
    pub state: BoxState,
}

/// `GET /v1/relay/boxes?after=N`: what changed since `after`. Never mentions
/// devices: the control plane doesn't know them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxFeed {
    /// Ask with this next time.
    pub seq: u64,
    /// `true` if `changes` is the whole list and replaces what the relay had.
    pub full: bool,
    /// The changes, oldest first.
    pub changes: Vec<BoxChange>,
}

/// `POST /v1/relay/report`: what a relay saw since its last report.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RelayReport {
    /// The relay's name, such as `relay1.zoomriya.net`.
    pub relay: String,
    /// Bytes each box sent and received through the relay since the last
    /// report, counted on the box's own connection.
    pub used: Vec<BoxUse>,
    /// Every box connected now, with how many of its devices are.
    pub connected: Vec<BoxConnected>,
    /// Bytes each box's devices sent and received through the TURN server
    /// beside this relay since the last report, from its own records.
    #[serde(default)]
    pub turn: Vec<BoxUse>,
    /// How the server is doing, for the GUI's Servers page.
    #[serde(default)]
    pub server: Option<ServerStats>,
}

/// How one of our servers is doing: sent with each relay's report, by the
/// second DNS server (`POST /v1/server/report`), and taken by the control
/// plane from its own machine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerStats {
    /// Its name, such as `relay1.zoomriya.net`.
    pub name: String,
    /// `control plane`, `relay` or `second DNS`.
    pub role: String,
    /// The program's version.
    pub version: String,
    /// When the program started, in seconds since 1970.
    pub started: u64,
    /// Seconds since the machine started.
    pub uptime: u64,
    /// Load over the last minute, and how many processors share it.
    pub load: f32,
    /// Processors.
    pub cpus: u32,
    /// Memory, in bytes.
    pub mem_total: u64,
    /// Memory in use (not available), in bytes.
    pub mem_used: u64,
    /// The disk holding the program's data, in bytes.
    pub disk_total: u64,
    /// Used on that disk, in bytes.
    pub disk_used: u64,
    /// Bytes received on the machine's network interfaces since it started.
    pub net_rx: u64,
    /// Bytes sent since it started.
    pub net_tx: u64,
    /// The TURN server beside a relay: `Some(true)` answering, `Some(false)`
    /// not answering, `None` none here.
    #[serde(default)]
    pub turn: Option<bool>,
}

/// One box's bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxUse {
    /// The box key.
    pub key: Key,
    /// Bytes.
    pub bytes: u64,
}

/// One box connected to a relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxConnected {
    /// The box key.
    pub key: Key,
    /// Its devices connected through this relay.
    pub devices: u32,
}

/// The control plane's answer to [`RelayReport`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayReportAnswer {
    /// Boxes past this month's allowance: slowed until the month ends.
    pub slow: Vec<Key>,
    /// How fast a slowed box may go, in bytes a second, each way.
    pub slow_bytes_per_second: u32,
}

/// `POST /v1/boxes/{name}/relay-use`: the box's use of our relays this month.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayUse {
    /// The month, such as `2026-11` (UTC).
    pub month: String,
    /// Bytes through our relays this month.
    pub used: u64,
    /// The month's allowance in bytes; 0 for none.
    pub allowance: u64,
    /// Past the allowance: slowed until the month ends.
    pub slowed: bool,
}

/// The month, such as `2026-11`, of a time in seconds since 1970 (UTC).
pub fn month_of(secs: u64) -> String {
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

/// One device the box removed, for the relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedDevice {
    /// The link key to refuse.
    pub link: Key,
    /// Until when, in seconds since 1970: the end of its last pass. After that
    /// the pass has run out anyway, and the relay forgets it.
    pub until: u64,
}

/// The box's list of removed devices, sealed with the box key
/// ([`zoomriya_keys::Purpose::RemovedDevices`]) and posted to
/// `/zoomriya/v1/removed` on the relay at once, and again on every reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedDevices {
    /// The box.
    #[serde(rename = "box")]
    pub box_key: Key,
    /// When the box made the list, in seconds since 1970. The relay only takes
    /// a list newer than the last one it took from this box.
    pub issued: u64,
    /// `true` if this is the box's whole list, replacing the last one;
    /// `false` to add to it.
    pub full: bool,
    /// The removed devices.
    pub devices: Vec<RemovedDevice>,
}

/// `POST /zoomriya/v1/enrol` on a family's own relay: the box introduces itself
/// with the one-time code shown when the relay was set up. Signed with the box key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolRequest {
    /// The code, as typed into Box Desktop.
    pub code: String,
    /// The box key.
    pub box_key: Key,
}

/// `/.well-known/zoomriya-connect.json`: everything a box needs from one
/// address typed into Box Desktop (docs/sharing-and-names.md, "Your own server").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WellKnown {
    /// The protocol version the server speaks.
    pub version: u32,
    /// The control plane's API, such as `https://connect.zoomriya.net`.
    pub control_plane: String,
    /// Where address records are published and fetched (`/pkarr`).
    pub address_records: String,
    /// The relays, in order of preference.
    pub relays: Vec<String>,
    /// The guest page, such as `https://share.zoomriya.net`.
    pub share: String,
    /// The zone box names live under, such as `zoomriya.app`.
    pub zone: String,
    /// The families' account page, such as `https://connect.zoomriya.net/account/`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_page: String,
    /// STUN servers (`stun:host:port`): the guests' relay, which also tells
    /// the box and laptops their public addresses for WebRTC.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stun: Vec<String>,
}

/// The STUN addresses of the guests' relay, from its `turn:` URLs: the same
/// server and port answers both. `turns:` (TLS) URLs have no STUN twin.
pub fn stun_from_turn(turn_urls: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for u in turn_urls {
        let Some(rest) = u.strip_prefix("turn:") else {
            continue;
        };
        let host = rest.split('?').next().unwrap_or(rest);
        let stun = format!("stun:{host}");
        if !out.contains(&stun) {
            out.push(stun);
        }
    }
    out
}

/// An admin note, sealed by the box key ([`zoomriya_keys::Purpose::AdminNote`]):
/// "the holder of this admin key is one of my admins". The control plane
/// checks the box's signature, so it never keeps a list of anyone's devices.
///
/// The admin key is an Ed25519 key the app keeps for this alone, locked by the
/// security chip like the link key. It isn't the chip's own key, which most
/// chips can't make in Ed25519.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminNote {
    /// The box that signed it.
    #[serde(rename = "box")]
    pub box_key: Key,
    /// The admin's key.
    pub admin: Key,
    /// When the box made it, in seconds since 1970.
    pub issued: u64,
}

/// `POST /v1/admin/stolen`: an admin marks the box stolen. Signed with the
/// admin key the note names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StolenRequest {
    /// The admin note, as sealed by the box.
    pub note: String,
}

/// `POST /v1/admin/move`: an admin moves the name to a new box (a new box, or
/// one erased and restored). Signed with the admin key the note names; the
/// note is from the old box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveRequest {
    /// The admin note from the old box.
    pub note: String,
    /// The new box's key.
    pub new_box_key: Key,
    /// The new box's DNS update key.
    pub new_dns_update_key: Key,
}

/// What the outside check found at the box's door.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoorVerdict {
    /// Guests can come straight in, and only reach the album viewer.
    Ok,
    /// Nothing answers: the router didn't open the door. Guests use WebRTC or the relay.
    NoDoor,
    /// Something answers, but it isn't the album viewer: close the door.
    WrongService,
    /// The sign-in page or an app can be reached through the door: close it at once.
    Leaks,
}

/// `POST /v1/boxes/{name}/check`: the answer. The control plane only ever
/// checks the asking box's own address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoorCheck {
    /// The address checked.
    pub address: String,
    /// The verdict.
    pub verdict: DoorVerdict,
    /// What happened, in plain words, for Box Desktop.
    pub detail: String,
}

/// `POST /zoomriya/v1/turn` on a relay, signed with the box key: credentials
/// for the TURN server beside that relay, for laptops (and later guests).
/// The username names the box, so the relay counts each box's TURN use.
pub const RELAY_TURN: &str = "/zoomriya/v1/turn";

/// `POST /v1/boxes/{name}/turn`: one-hour credentials for the guests' relay,
/// asked for by the box after it checked a guest's token. The box passes
/// them to the guest in the setup messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCredentials {
    /// The guests' relay, as WebRTC wants it (`turn:` and `turns:` URLs).
    pub urls: Vec<String>,
    /// The username: when it runs out, and a random part.
    pub username: String,
    /// The password for that username.
    pub credential: String,
    /// Seconds it works for.
    pub ttl: u64,
}

/// A setup message between a guest's browser and the box, passed on by the
/// control plane without reading it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalMessage {
    /// Which guest: the control plane numbers them.
    pub session: u64,
    /// The message itself, opaque to the control plane.
    pub data: String,
}

/// What the control plane tells the box about a guest, besides their messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SignalEvent {
    /// A guest's message.
    Message(SignalMessage),
    /// The guest left, or took too long.
    Gone {
        /// Which guest.
        session: u64,
    },
}

/// `GET /v1/zone`: everything DNS answers, for the second DNS server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneCopy {
    /// Changes whenever anything does.
    pub serial: u32,
    /// Each box name's entries.
    pub names: Vec<ZoneName>,
    /// Address records: box key and the record, base64url.
    pub records: Vec<(Key, String)>,
}

/// One box name in a [`ZoneCopy`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneName {
    /// The name, such as `maplehouse`.
    pub name: String,
    /// Where it points.
    pub home: HomeAddrs,
    /// The door's IPv4 address, while open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_v4: Option<Ipv4Addr>,
    /// The door's IPv6 address, while open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_v6: Option<Ipv6Addr>,
    /// When the door drops out.
    pub door_until: u64,
    /// Certificate-check values for the box's name.
    #[serde(default)]
    pub acme_box: Vec<String>,
    /// Certificate-check values for the door's name.
    #[serde(default)]
    pub acme_door: Vec<String>,
    /// When the check values drop out.
    pub acme_until: u64,
}

#[cfg(test)]
mod tests {
    #[test]
    fn months() {
        assert_eq!(super::month_of(0), "1970-01");
        assert_eq!(super::month_of(1_798_761_600 - 1), "2026-12");
        assert_eq!(super::month_of(1_798_761_600), "2027-01");
        assert_eq!(super::month_of(1_709_164_800), "2024-02");
        assert_eq!(super::month_of(1_709_251_200), "2024-03");
    }

    use super::*;

    #[test]
    fn feed_reads_as_documented() {
        let key = zoomriya_keys::SecretKey::generate().public();
        let json = format!(
            r#"{{"seq":4,"full":false,"changes":[{{"key":"{}","state":"stolen"}}]}}"#,
            key.to_z32()
        );
        let feed: BoxFeed = serde_json::from_str(&json).expect("reads");
        assert_eq!(feed.changes[0].key, Key(key));
        assert_eq!(feed.changes[0].state, BoxState::Stolen);
        assert_eq!(serde_json::to_string(&feed).expect("writes"), json);
    }
}
