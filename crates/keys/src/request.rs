//! Signed HTTP requests from the box to the control plane or a relay.
//!
//! The box signs the method, path, time and a hash of the body with the key
//! the server knows: the box key, or for DNS changes the DNS update key. The
//! header is `Zoomriya-Signature: <key>.<seconds since 1970>.<signature>`.

use data_encoding::{BASE64URL_NOPAD, HEXLOWER};
use sha2::{Digest, Sha256};

use crate::{KeyError, PublicKey, Purpose, SecretKey, decode_signature, sign, verify};

/// The header that carries a request's signature.
pub const REQUEST_SIGNATURE_HEADER: &str = "zoomriya-signature";

fn message(method: &str, path: &str, time: u64, body: &[u8]) -> Vec<u8> {
    let hash = HEXLOWER.encode(&Sha256::digest(body));
    format!(
        "{}\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        path,
        time,
        hash
    )
    .into_bytes()
}

/// Signs a request; returns the header's value.
pub fn sign_request(key: &SecretKey, method: &str, path: &str, time: u64, body: &[u8]) -> String {
    let sig = sign(key, Purpose::Request, &message(method, path, time, body));
    format!(
        "{}.{}.{}",
        key.public().to_z32(),
        time,
        BASE64URL_NOPAD.encode(&sig.to_bytes())
    )
}

/// A request signature read from its header, not yet checked.
#[derive(Debug, Clone)]
pub struct RequestSignature {
    /// The key that says it signed.
    pub key: PublicKey,
    /// When it says it signed, in seconds since 1970.
    pub time: u64,
    sig: crate::Signature,
}

impl RequestSignature {
    /// Reads the header's value.
    pub fn parse(header: &str) -> Result<Self, KeyError> {
        if header.len() > 256 {
            return Err(KeyError::Malformed);
        }
        let mut parts = header.splitn(3, '.');
        let (Some(key), Some(time), Some(sig)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(KeyError::Malformed);
        };
        Ok(Self {
            key: PublicKey::from_z32(key).map_err(|_| KeyError::Malformed)?,
            time: time.parse().map_err(|_| KeyError::Malformed)?,
            sig: decode_signature(sig)?,
        })
    }

    /// Checks the signature over this request, and that it was made within
    /// `skew` seconds of `now`. A replay inside that window repeats a request
    /// the box already made; every signed request is safe to repeat.
    pub fn verify(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        now: u64,
        skew: u64,
    ) -> Result<(), KeyError> {
        if self.time.abs_diff(now) > skew {
            return Err(KeyError::BadSignature);
        }
        verify(
            &self.key,
            Purpose::Request,
            &message(method, path, self.time, body),
            &self.sig,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tamper() {
        let k = SecretKey::generate();
        let h = sign_request(&k, "put", "/v1/boxes/maple/home", 1000, b"{}");
        let s = RequestSignature::parse(&h).expect("parses");
        assert_eq!(s.key, k.public());
        assert!(
            s.verify("PUT", "/v1/boxes/maple/home", b"{}", 1000, 600)
                .is_ok()
        );
        assert!(
            s.verify("PUT", "/v1/boxes/maple/home", b"{ }", 1000, 600)
                .is_err()
        );
        assert!(
            s.verify("PUT", "/v1/boxes/other/home", b"{}", 1000, 600)
                .is_err()
        );
        assert!(
            s.verify("DELETE", "/v1/boxes/maple/home", b"{}", 1000, 600)
                .is_err()
        );
        assert!(
            s.verify("PUT", "/v1/boxes/maple/home", b"{}", 1601, 600)
                .is_err()
        );
        assert!(RequestSignature::parse("a.b").is_err());
    }
}
