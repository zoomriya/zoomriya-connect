//! Messages shared by the link, connectd, the relay and the control plane.
//!
//! Every request carries a protocol version. Servers accept the current
//! version and the one before; older ones get [`ErrorBody`] with code
//! `please_update` (docs/architecture.md, "Versions and updates").

use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};
use zoomriya_keys::Key;

pub mod addrs;
pub mod link;
pub mod names;

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
