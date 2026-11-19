//! Device keys: P-256 keys made in a phone's security chip (Secure Enclave,
//! Keystore) or by the browser extension, which never leave it. Most chips
//! can't hold iroh's Ed25519 keys, so the device key signs each new link key
//! instead, and the box pins the device key (docs/architecture.md, "The link").

use data_encoding::BASE64URL_NOPAD;
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};

use crate::{KeyError, PublicKey};

/// What a device key signs to vouch for a link key.
pub fn link_key_message(link: &PublicKey) -> Vec<u8> {
    let mut m = b"zoomriya-connect\0link-key/v1\0".to_vec();
    m.extend_from_slice(link.as_bytes());
    m
}

/// A device's P-256 public key, as the box pins it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceKey(VerifyingKey);

impl DeviceKey {
    /// Reads a SEC1 key (compressed or not), base64url.
    pub fn from_b64(s: &str) -> Result<Self, KeyError> {
        let bytes = BASE64URL_NOPAD
            .decode(s.trim().as_bytes())
            .map_err(|_| KeyError::Malformed)?;
        VerifyingKey::from_sec1_bytes(&bytes)
            .map(DeviceKey)
            .map_err(|_| KeyError::Malformed)
    }

    /// The key, SEC1 compressed, base64url: how the box stores it.
    pub fn to_b64(&self) -> String {
        BASE64URL_NOPAD.encode(self.0.to_sec1_point(true).as_bytes())
    }

    /// Checks the device key's signature over a link key. Takes DER (what the
    /// phones' chips give) or the raw 64 bytes (what browsers give), base64url.
    pub fn verify_link(&self, link: &PublicKey, sig_b64: &str) -> Result<(), KeyError> {
        let bytes = BASE64URL_NOPAD
            .decode(sig_b64.trim().as_bytes())
            .map_err(|_| KeyError::Malformed)?;
        let sig = if bytes.len() == 64 {
            Signature::from_slice(&bytes)
        } else {
            Signature::from_der(&bytes)
        }
        .map_err(|_| KeyError::Malformed)?;
        self.0
            .verify(&link_key_message(link), &sig)
            .map_err(|_| KeyError::BadSignature)
    }
}

/// A P-256 key like a phone's security chip holds, for tests and the test
/// phone only: the real one never leaves the chip.
#[doc(hidden)]
pub fn new_test_chip() -> p256::ecdsa::SigningKey {
    loop {
        let mut bytes = [0u8; 32];
        if getrandom::fill(&mut bytes).is_ok()
            && let Ok(k) = p256::ecdsa::SigningKey::from_slice(&bytes)
        {
            return k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretKey;
    use p256::ecdsa::signature::Signer;

    #[test]
    fn device_vouches_for_link_keys() {
        let chip = crate::device::new_test_chip();
        let device = DeviceKey::from_b64(
            &BASE64URL_NOPAD.encode(chip.verifying_key().to_sec1_point(false).as_bytes()),
        )
        .expect("reads");
        assert_eq!(
            DeviceKey::from_b64(&device.to_b64()).expect("again"),
            device
        );

        let link = SecretKey::generate().public();
        let sig: Signature = chip.sign(&link_key_message(&link));
        // As a phone's chip gives it, and as a browser does.
        let der = BASE64URL_NOPAD.encode(sig.to_der().as_bytes());
        let raw = BASE64URL_NOPAD.encode(&sig.to_bytes());
        assert_eq!(device.verify_link(&link, &der), Ok(()));
        assert_eq!(device.verify_link(&link, &raw), Ok(()));

        let other = SecretKey::generate().public();
        assert_eq!(
            device.verify_link(&other, &der),
            Err(KeyError::BadSignature)
        );
        assert_eq!(device.verify_link(&link, "!!"), Err(KeyError::Malformed));
        let stranger = crate::device::new_test_chip();
        let theirs: Signature = stranger.sign(&link_key_message(&link));
        assert_eq!(
            device.verify_link(&link, &BASE64URL_NOPAD.encode(&theirs.to_bytes())),
            Err(KeyError::BadSignature)
        );
    }
}
