//! Keys for Direct+.
//!
//! The box key is the box's iroh endpoint key: an Ed25519 key made at first
//! setup that never leaves the box. Devices pin it at pairing. Each device
//! has a P-256 device key in its security chip, which vouches for its iroh
//! link key ([`DeviceKey`]); the box pins the device key.

use std::fmt;

use data_encoding::BASE64URL_NOPAD;
use serde::{Deserialize, Serialize};

pub use iroh_base::{PublicKey, SecretKey, Signature};

mod device;
pub mod pake;
pub mod record;

pub use device::{DeviceKey, link_key_message, new_test_chip};

/// Why a key or signature was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// Not in the expected shape: bad base64, wrong length.
    #[error("malformed")]
    Malformed,
    /// The signature doesn't match.
    #[error("bad signature")]
    BadSignature,
}

/// Reads a base64url signature.
pub fn decode_signature(s: &str) -> Result<Signature, KeyError> {
    let bytes = BASE64URL_NOPAD
        .decode(s.as_bytes())
        .map_err(|_| KeyError::Malformed)?;
    let bytes: [u8; 64] = bytes.try_into().map_err(|_| KeyError::Malformed)?;
    Ok(Signature::from_bytes(&bytes))
}

/// Writes a key the way ZOOMRIYA shows keys everywhere: z-base-32, as
/// in address records.
pub fn key_to_string(key: &PublicKey) -> String {
    key.to_z32()
}

/// Reads a key written by [`key_to_string`].
pub fn key_from_str(s: &str) -> Result<PublicKey, KeyError> {
    PublicKey::from_z32(s).map_err(|_| KeyError::Malformed)
}

/// Serde helpers that write a [`PublicKey`] as z-base-32.
pub mod z32 {
    use super::*;
    use serde::{Deserializer, Serializer};

    /// Writes the key as z-base-32.
    pub fn serialize<S: Serializer>(key: &PublicKey, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&key.to_z32())
    }

    /// Reads a z-base-32 key.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PublicKey, D::Error> {
        let s = String::deserialize(d)?;
        PublicKey::from_z32(&s).map_err(serde::de::Error::custom)
    }
}

/// A [`PublicKey`] that serialises as z-base-32, for maps and lists.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Key(#[serde(with = "z32")] pub PublicKey);

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.fmt_short())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_z32())
    }
}

impl From<PublicKey> for Key {
    fn from(k: PublicKey) -> Self {
        Key(k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_as_text() {
        let k = SecretKey::generate().public();
        assert_eq!(key_from_str(&key_to_string(&k)), Ok(k));
        assert_eq!(key_from_str("not a key"), Err(KeyError::Malformed));
        let json = serde_json::to_string(&Key(k)).expect("json");
        assert_eq!(json, format!("\"{}\"", k.to_z32()));
    }
}
