//! Messages shared by the phone and laptop apps' link and connectd.

use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

pub mod addrs;
pub mod laptop;
pub mod link;

/// What connectd says when it refuses a request. `code` is stable, for
/// programs; `message` is plain words, for Box Desktop to show as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// A stable code, such as `name_taken`.
    pub code: String,
    /// What to show the family.
    pub message: String,
}

/// The box's addresses on its home network. Private addresses
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
