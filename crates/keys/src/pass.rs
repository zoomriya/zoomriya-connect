//! Passes: the box's signed note that a link key belongs to it, for a while.
//!
//! The app sends its pass to the relay as the bearer token
//! (`RelayMap::with_auth_token`). The relay checks it by itself, so it never
//! asks the control plane about devices (docs/how-it-works.md, "The box hands
//! out passes"). A pass gets a device through the relay, never into the box.

use serde::{Deserialize, Serialize};

use crate::{KeyError, PublicKey, Purpose, SecretKey, open, seal, z32};

/// Every pass token starts with this, so a wrong kind of token is refused
/// before any work.
pub const PASS_PREFIX: &str = "zp1.";

/// Longest pass token we read. A real one is about 300 characters.
pub const MAX_PASS_TOKEN_LEN: usize = 1024;

/// A pass, as signed by the box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pass {
    /// The box that made the pass, and whose key signs it.
    #[serde(rename = "box", with = "z32")]
    pub box_key: PublicKey,
    /// The device's current link key: its iroh endpoint ID.
    #[serde(rename = "link", with = "z32")]
    pub link_key: PublicKey,
    /// Valid from, in seconds since 1970 (UTC).
    #[serde(rename = "nbf")]
    pub not_before: u64,
    /// Valid until, in seconds since 1970 (UTC).
    #[serde(rename = "exp")]
    pub not_after: u64,
}

/// Why a relay turned a pass away. Counted per reason, never naming the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum PassRefused {
    /// No token at all.
    #[error("no pass")]
    Missing,
    /// Not a pass, or damaged.
    #[error("malformed pass")]
    Malformed,
    /// Not signed by the box it names.
    #[error("pass not signed by its box")]
    BadSignature,
    /// The pass is for another link key than the one that connected.
    #[error("pass is for another device")]
    WrongDevice,
    /// Starts in the future, beyond the clock allowance.
    #[error("pass not valid yet")]
    NotYetValid,
    /// Ran out, beyond the clock allowance.
    #[error("pass expired")]
    Expired,
    /// Longer than the relay accepts.
    #[error("pass lasts too long")]
    TooLong,
    /// The box isn't registered with this relay.
    #[error("unknown box")]
    UnknownBox,
    /// The box removed this device.
    #[error("device removed")]
    Removed,
    /// An admin marked the box stolen.
    #[error("box marked stolen")]
    Stolen,
}

impl PassRefused {
    /// A short, stable name for metrics.
    pub fn as_str(self) -> &'static str {
        match self {
            PassRefused::Missing => "missing",
            PassRefused::Malformed => "malformed",
            PassRefused::BadSignature => "bad_signature",
            PassRefused::WrongDevice => "wrong_device",
            PassRefused::NotYetValid => "not_yet_valid",
            PassRefused::Expired => "expired",
            PassRefused::TooLong => "too_long",
            PassRefused::UnknownBox => "unknown_box",
            PassRefused::Removed => "removed",
            PassRefused::Stolen => "stolen",
        }
    }
}

impl Pass {
    /// Signs the pass with the box key. Fails if `box_secret` isn't the key
    /// the pass names.
    pub fn sign(&self, box_secret: &SecretKey) -> Result<String, KeyError> {
        if box_secret.public() != self.box_key {
            return Err(KeyError::WrongSigner);
        }
        Ok(format!(
            "{PASS_PREFIX}{}",
            seal(box_secret, Purpose::Pass, self)
        ))
    }

    /// Reads a token and checks that the box it names signed it. Says nothing
    /// yet about dates or devices: see [`Pass::check`].
    pub fn open(token: &str) -> Result<Pass, PassRefused> {
        if token.len() > MAX_PASS_TOKEN_LEN {
            return Err(PassRefused::Malformed);
        }
        let rest = token
            .strip_prefix(PASS_PREFIX)
            .ok_or(PassRefused::Malformed)?;
        open(Purpose::Pass, rest, MAX_PASS_TOKEN_LEN, |p: &Pass| {
            p.box_key
        })
        .map_err(|e| match e {
            KeyError::BadSignature | KeyError::WrongSigner => PassRefused::BadSignature,
            KeyError::Malformed => PassRefused::Malformed,
        })
    }

    /// Checks the pass for the link key that connected, at `now` (seconds since
    /// 1970), allowing `skew` seconds of clock difference and passes of at most
    /// `max_len` seconds.
    pub fn check(
        &self,
        link_key: &PublicKey,
        now: u64,
        skew: u64,
        max_len: u64,
    ) -> Result<(), PassRefused> {
        if &self.link_key != link_key {
            return Err(PassRefused::WrongDevice);
        }
        if self.not_after < self.not_before || self.not_after - self.not_before > max_len {
            return Err(PassRefused::TooLong);
        }
        if now.saturating_add(skew) < self.not_before {
            return Err(PassRefused::NotYetValid);
        }
        if now > self.not_after.saturating_add(skew) {
            return Err(PassRefused::Expired);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    const NOW: u64 = 1_800_000_000;

    fn pass(box_key: &SecretKey, link: &SecretKey) -> Pass {
        Pass {
            box_key: box_key.public(),
            link_key: link.public(),
            not_before: NOW,
            not_after: NOW + 30 * DAY,
        }
    }

    #[test]
    fn a_good_pass() {
        let b = SecretKey::generate();
        let link = SecretKey::generate();
        let p = pass(&b, &link);
        let token = p.sign(&b).expect("signs");
        assert!(token.starts_with(PASS_PREFIX));
        assert!(token.len() < 400, "{}", token.len());
        let opened = Pass::open(&token).expect("opens");
        assert_eq!(opened, p);
        assert_eq!(
            opened.check(&link.public(), NOW + DAY, 600, 31 * DAY),
            Ok(())
        );
    }

    #[test]
    fn only_its_box_can_sign() {
        let b = SecretKey::generate();
        let other = SecretKey::generate();
        let link = SecretKey::generate();
        assert_eq!(pass(&b, &link).sign(&other), Err(KeyError::WrongSigner));
    }

    #[test]
    fn tampering_is_caught() {
        let b = SecretKey::generate();
        let link = SecretKey::generate();
        let token = pass(&b, &link).sign(&b).expect("signs");
        // Swap in a body naming another device, keeping the signature.
        let other = pass(&b, &SecretKey::generate());
        let other_token = other.sign(&b).expect("signs");
        let (_, sig) = token.rsplit_once('.').expect("dot");
        let (other_body, _) = other_token.rsplit_once('.').expect("dot");
        let forged = format!("{other_body}.{sig}");
        assert_eq!(Pass::open(&forged), Err(PassRefused::BadSignature));
        assert_eq!(Pass::open("zp1.nonsense"), Err(PassRefused::Malformed));
        assert_eq!(Pass::open("Bearer x"), Err(PassRefused::Malformed));
        assert_eq!(Pass::open(&"zp1.".repeat(400)), Err(PassRefused::Malformed));
    }

    #[test]
    fn dates_and_devices() {
        let b = SecretKey::generate();
        let link = SecretKey::generate();
        let p = pass(&b, &link);
        let max = 31 * DAY;
        assert_eq!(
            p.check(&SecretKey::generate().public(), NOW, 600, max),
            Err(PassRefused::WrongDevice)
        );
        // Ten minutes' allowance either side.
        assert_eq!(p.check(&link.public(), NOW - 600, 600, max), Ok(()));
        assert_eq!(
            p.check(&link.public(), NOW - 601, 600, max),
            Err(PassRefused::NotYetValid)
        );
        assert_eq!(
            p.check(&link.public(), NOW + 30 * DAY + 600, 600, max),
            Ok(())
        );
        assert_eq!(
            p.check(&link.public(), NOW + 30 * DAY + 601, 600, max),
            Err(PassRefused::Expired)
        );
        let long = Pass {
            not_after: NOW + 90 * DAY,
            ..p.clone()
        };
        assert_eq!(
            long.check(&link.public(), NOW, 600, max),
            Err(PassRefused::TooLong)
        );
        let backwards = Pass {
            not_after: NOW - 1,
            ..p
        };
        assert_eq!(
            backwards.check(&link.public(), NOW, 600, max),
            Err(PassRefused::TooLong)
        );
    }
}
