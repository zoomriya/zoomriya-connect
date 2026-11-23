//! The device key: P-256, which vouches for the link key (the device's iroh
//! key) at pairing. The box pins it.
//!
//! Phones keep it in their security chip. The laptop app keeps it in a file
//! only its user can read, until the macOS keychain, Windows' TPM and Linux's
//! secret service are wired in (docs/direct-plus.md, §10).

use data_encoding::BASE64URL_NOPAD;
use p256::ecdsa::{Signature, SigningKey, signature::Signer as _};
use zoomriya_keys::{PublicKey, link_key_message};

/// Something holding a device key.
pub trait Signer: Send + Sync {
    /// The public key, SEC1 compressed, base64url, as the box pins it.
    fn public_b64(&self) -> String;
    /// The signature over a link key, DER, base64url.
    fn sign_link(&self, link: &PublicKey) -> String;
}

/// A device key held in memory, read from and written to a file by the app.
pub struct SoftKey(SigningKey);

impl std::fmt::Debug for SoftKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SoftKey")
    }
}

impl SoftKey {
    /// A new key.
    pub fn generate() -> anyhow::Result<Self> {
        for _ in 0..16 {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
            if let Ok(k) = SigningKey::from_slice(&bytes) {
                return Ok(Self(k));
            }
        }
        anyhow::bail!("could not make a device key")
    }

    /// The key from its 32 secret bytes.
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        SigningKey::from_slice(bytes)
            .map(Self)
            .map_err(|_| anyhow::anyhow!("not a device key"))
    }

    /// Its 32 secret bytes, for the app's own file.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }
}

impl Signer for SoftKey {
    fn public_b64(&self) -> String {
        BASE64URL_NOPAD.encode(self.0.verifying_key().to_sec1_point(true).as_bytes())
    }

    fn sign_link(&self, link: &PublicKey) -> String {
        let s: Signature = self.0.sign(&link_key_message(link));
        BASE64URL_NOPAD.encode(s.to_der().as_bytes())
    }
}
