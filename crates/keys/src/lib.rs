//! Keys and signatures for ZOOMRIYA Connect.
//!
//! The box key is the box's iroh endpoint key: an Ed25519 key made at first
//! setup that never leaves the box. Everything it signs says what it's for
//! ([`Purpose`]), so one kind of signature can't be passed off as another
//! (docs/architecture.md, "connectd").
//!
//! This crate holds what both sides need to agree on byte for byte: how a
//! signed note is framed, the [pass](Pass) a device shows the relay, and the
//! signature on requests to the control plane. Device keys in the phone's
//! security chip are the link's business and aren't here.

use std::fmt;

use data_encoding::BASE64URL_NOPAD;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub use iroh_base::{PublicKey, SecretKey, Signature};

mod device;
mod pass;
mod request;

pub use device::{DeviceKey, link_key_message};
pub use pass::{MAX_PASS_TOKEN_LEN, PASS_PREFIX, Pass, PassRefused};
pub use request::{REQUEST_SIGNATURE_HEADER, RequestSignature, sign_request};

/// What a signature is for. The purpose is part of what's signed, so a
/// signature made for one purpose never verifies for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A pass: "this link key belongs to this box until then". Signed by the box key.
    Pass,
    /// The box's list of removed devices, for the relay. Signed by the box key.
    RemovedDevices,
    /// A request to the control plane or a relay's own API.
    Request,
    /// An admin note: lets one admin's phone move the box's name or mark it
    /// stolen. Signed by the box key.
    AdminNote,
}

impl Purpose {
    fn context(self) -> &'static [u8] {
        match self {
            Purpose::Pass => b"pass/v1",
            Purpose::RemovedDevices => b"removed-devices/v1",
            Purpose::Request => b"request/v1",
            Purpose::AdminNote => b"admin-note/v1",
        }
    }
}

/// Every signed message starts with this, then the purpose, then the message.
const DOMAIN: &[u8] = b"zoomriya-connect\0";

fn framed(purpose: Purpose, msg: &[u8]) -> Vec<u8> {
    let ctx = purpose.context();
    let mut out = Vec::with_capacity(DOMAIN.len() + ctx.len() + 1 + msg.len());
    out.extend_from_slice(DOMAIN);
    out.extend_from_slice(ctx);
    out.push(0);
    out.extend_from_slice(msg);
    out
}

/// Signs `msg` for `purpose`.
pub fn sign(key: &SecretKey, purpose: Purpose, msg: &[u8]) -> Signature {
    key.sign(&framed(purpose, msg))
}

/// Checks a signature made by [`sign`] for the same purpose.
pub fn verify(
    key: &PublicKey,
    purpose: Purpose,
    msg: &[u8],
    sig: &Signature,
) -> Result<(), KeyError> {
    key.verify(&framed(purpose, msg), sig)
        .map_err(|_| KeyError::BadSignature)
}

/// Why a key, signature or signed note was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// Not in the expected shape: wrong prefix, bad base64, bad JSON, too long.
    #[error("malformed")]
    Malformed,
    /// The signature doesn't match.
    #[error("bad signature")]
    BadSignature,
    /// The key that signed isn't the key named inside the note.
    #[error("signed by the wrong key")]
    WrongSigner,
}

/// A JSON body and its signature, written `<body>.<signature>`, both base64url
/// without padding. Used for passes, removed-device lists and admin notes.
pub fn seal<T: Serialize>(key: &SecretKey, purpose: Purpose, body: &T) -> String {
    // Serialising our own plain structs can't fail.
    let json = serde_json::to_vec(body).unwrap_or_default();
    let sig = sign(key, purpose, &json);
    format!(
        "{}.{}",
        BASE64URL_NOPAD.encode(&json),
        BASE64URL_NOPAD.encode(&sig.to_bytes())
    )
}

/// Opens a note made by [`seal`]. `signer` picks the key that must have
/// signed it, usually a key named inside the note itself; the body is only
/// returned once that key's signature checks out.
pub fn open<T: DeserializeOwned>(
    purpose: Purpose,
    sealed: &str,
    max_len: usize,
    signer: impl FnOnce(&T) -> PublicKey,
) -> Result<T, KeyError> {
    if sealed.len() > max_len {
        return Err(KeyError::Malformed);
    }
    let (body, sig) = sealed.split_once('.').ok_or(KeyError::Malformed)?;
    let json = BASE64URL_NOPAD
        .decode(body.as_bytes())
        .map_err(|_| KeyError::Malformed)?;
    let sig = decode_signature(sig)?;
    let value: T = serde_json::from_slice(&json).map_err(|_| KeyError::Malformed)?;
    verify(&signer(&value), purpose, &json, &sig)?;
    Ok(value)
}

/// Reads a base64url signature.
pub fn decode_signature(s: &str) -> Result<Signature, KeyError> {
    let bytes = BASE64URL_NOPAD
        .decode(s.as_bytes())
        .map_err(|_| KeyError::Malformed)?;
    let bytes: [u8; 64] = bytes.try_into().map_err(|_| KeyError::Malformed)?;
    Ok(Signature::from_bytes(&bytes))
}

/// Writes a key the way ZOOMRIYA Connect shows keys everywhere: z-base-32, as
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
    fn purposes_dont_cross() {
        let key = SecretKey::generate();
        let sig = sign(&key, Purpose::Pass, b"hello");
        assert!(verify(&key.public(), Purpose::Pass, b"hello", &sig).is_ok());
        assert_eq!(
            verify(&key.public(), Purpose::AdminNote, b"hello", &sig),
            Err(KeyError::BadSignature)
        );
        assert_eq!(
            verify(&key.public(), Purpose::Pass, b"hellO", &sig),
            Err(KeyError::BadSignature)
        );
    }

    #[test]
    fn seal_and_open() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Note {
            #[serde(with = "z32")]
            by: PublicKey,
            n: u32,
        }
        let key = SecretKey::generate();
        let note = Note {
            by: key.public(),
            n: 7,
        };
        let sealed = seal(&key, Purpose::RemovedDevices, &note);
        let opened: Note =
            open(Purpose::RemovedDevices, &sealed, 4096, |n: &Note| n.by).expect("opens");
        assert_eq!(opened, note);

        // Someone else claiming to have written it.
        let other = SecretKey::generate();
        let forged = seal(
            &other,
            Purpose::RemovedDevices,
            &Note {
                by: key.public(),
                n: 8,
            },
        );
        assert_eq!(
            open(Purpose::RemovedDevices, &forged, 4096, |n: &Note| n.by),
            Err::<Note, _>(KeyError::BadSignature)
        );
        assert_eq!(
            open(Purpose::RemovedDevices, &sealed, 10, |n: &Note| n.by),
            Err::<Note, _>(KeyError::Malformed)
        );
    }
}
