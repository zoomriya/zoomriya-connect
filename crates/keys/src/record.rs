//! The box's address record in the BitTorrent DHT (docs/direct-plus.md §3):
//! where paired devices find the box from away, with no server.
//!
//! | Part | How |
//! |---|---|
//! | [`RecordSecret`] | 32 random bytes the box makes once and gives each device at pairing |
//! | The DHT key | An Ed25519 key derived from the secret and the day ([`RecordSecret::signing_seed`]): the record moves every day, and only paired devices can find it |
//! | The value | Sealed with ChaCha20-Poly1305 under a key from the same secret: only paired devices can read it, and nobody can change it unseen |
//!
//! The value is compact, to stay well under the DHT's 1000 bytes:
//! version (1), written at (8, seconds since 1970), count (1), then each
//! address as family (1: 4 or 6), the address (4 or 16) and the port (2).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use data_encoding::BASE64URL_NOPAD;
use hkdf::Hkdf;
use sha2::Sha256;

use crate::KeyError;

/// Most addresses in a record.
pub const MAX_ADDRS: usize = 8;
const VERSION: u8 = 1;
const AAD: &[u8] = b"zoomriya box record v1";

/// The secret behind the box's record.
#[derive(Clone, PartialEq, Eq)]
pub struct RecordSecret([u8; 32]);

impl std::fmt::Debug for RecordSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecordSecret")
    }
}

/// The day number: whole days since 1970.
pub fn day(unix_seconds: u64) -> u64 {
    unix_seconds / 86_400
}

impl RecordSecret {
    /// A new secret.
    pub fn generate() -> Result<Self, KeyError> {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|_| KeyError::Malformed)?;
        Ok(Self(b))
    }

    /// From its 32 bytes.
    pub fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    /// Its 32 bytes, for the box's own file.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// As text, for pairing.
    pub fn to_b64(&self) -> String {
        BASE64URL_NOPAD.encode(&self.0)
    }

    /// From text.
    pub fn from_b64(s: &str) -> Result<Self, KeyError> {
        let b = BASE64URL_NOPAD
            .decode(s.trim().as_bytes())
            .map_err(|_| KeyError::Malformed)?;
        Ok(Self(b.try_into().map_err(|_| KeyError::Malformed)?))
    }

    fn derive(&self, info: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        Hkdf::<Sha256>::new(None, &self.0)
            .expand(info, &mut out)
            .unwrap_or_else(|_| unreachable!("32 bytes is a valid HKDF length"));
        out
    }

    /// The Ed25519 seed for `day`'s DHT key.
    pub fn signing_seed(&self, day: u64) -> [u8; 32] {
        let mut info = b"zoomriya box record key v1\0".to_vec();
        info.extend_from_slice(&day.to_be_bytes());
        self.derive(&info)
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(&self.derive(b"zoomriya box record seal v1").into())
    }

    /// Seals the addresses (at most [`MAX_ADDRS`]) written at `at`.
    pub fn seal(&self, addrs: &[SocketAddr], at: u64) -> Result<Vec<u8>, KeyError> {
        let mut plain = vec![VERSION];
        plain.extend_from_slice(&at.to_be_bytes());
        let addrs = &addrs[..addrs.len().min(MAX_ADDRS)];
        plain.push(addrs.len() as u8);
        for a in addrs {
            match a.ip() {
                IpAddr::V4(ip) => {
                    plain.push(4);
                    plain.extend_from_slice(&ip.octets());
                }
                IpAddr::V6(ip) => {
                    plain.push(6);
                    plain.extend_from_slice(&ip.octets());
                }
            }
            plain.extend_from_slice(&a.port().to_be_bytes());
        }
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| KeyError::Malformed)?;
        let sealed = self
            .cipher()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: AAD,
                },
            )
            .map_err(|_| KeyError::Malformed)?;
        let mut out = nonce.to_vec();
        out.extend(sealed);
        Ok(out)
    }

    /// Opens a record: its addresses and when it was written.
    pub fn open(&self, value: &[u8]) -> Result<(Vec<SocketAddr>, u64), KeyError> {
        if value.len() < 12 {
            return Err(KeyError::Malformed);
        }
        let (nonce, sealed) = value.split_at(12);
        let plain = self
            .cipher()
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: AAD,
                },
            )
            .map_err(|_| KeyError::BadSignature)?;
        let take = |p: &mut &[u8], n: usize| -> Result<Vec<u8>, KeyError> {
            if p.len() < n {
                return Err(KeyError::Malformed);
            }
            let (a, b) = p.split_at(n);
            *p = b;
            Ok(a.to_vec())
        };
        let mut p = plain.as_slice();
        if take(&mut p, 1)?[0] != VERSION {
            return Err(KeyError::Malformed);
        }
        let at = u64::from_be_bytes(
            take(&mut p, 8)?
                .try_into()
                .map_err(|_| KeyError::Malformed)?,
        );
        let n = take(&mut p, 1)?[0] as usize;
        let mut addrs = Vec::with_capacity(n);
        for _ in 0..n.min(MAX_ADDRS) {
            let ip = match take(&mut p, 1)?[0] {
                4 => IpAddr::V4(Ipv4Addr::from(
                    <[u8; 4]>::try_from(take(&mut p, 4)?).map_err(|_| KeyError::Malformed)?,
                )),
                6 => IpAddr::V6(Ipv6Addr::from(
                    <[u8; 16]>::try_from(take(&mut p, 16)?).map_err(|_| KeyError::Malformed)?,
                )),
                _ => return Err(KeyError::Malformed),
            };
            let port = u16::from_be_bytes(
                take(&mut p, 2)?
                    .try_into()
                    .map_err(|_| KeyError::Malformed)?,
            );
            addrs.push(SocketAddr::new(ip, port));
        }
        Ok((addrs, at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_for_paired_devices_only() {
        let s = RecordSecret::generate().expect("secret");
        let addrs: Vec<SocketAddr> = vec![
            "203.0.113.7:443".parse().expect("addr"),
            "[2001:db8::7]:443".parse().expect("addr"),
        ];
        let v = s.seal(&addrs, 1_800_000_000).expect("sealed");
        assert!(v.len() < 100, "{}", v.len());
        assert_eq!(s.open(&v), Ok((addrs.clone(), 1_800_000_000)));
        // Another box's secret can't read it; a changed byte fails.
        let other = RecordSecret::generate().expect("secret");
        assert_eq!(other.open(&v).map(|_| ()), Err(KeyError::BadSignature));
        let mut bad = v.clone();
        bad[20] ^= 1;
        assert!(s.open(&bad).is_err());
        // As text, for pairing.
        assert_eq!(RecordSecret::from_b64(&s.to_b64()), Ok(s.clone()));
        // At most eight addresses, and still small.
        let many: Vec<SocketAddr> = (0..20)
            .map(|i| {
                SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, i)),
                    443,
                )
            })
            .collect();
        let v = s.seal(&many, 1).expect("sealed");
        assert!(v.len() < 250, "{}", v.len());
        assert_eq!(s.open(&v).expect("opens").0.len(), MAX_ADDRS);
    }

    #[test]
    fn the_key_moves_every_day() {
        let s = RecordSecret::from_bytes([7; 32]);
        assert_eq!(s.signing_seed(20_000), s.signing_seed(20_000));
        assert_ne!(s.signing_seed(20_000), s.signing_seed(20_001));
        assert_ne!(
            s.signing_seed(20_000),
            RecordSecret::from_bytes([8; 32]).signing_seed(20_000)
        );
        assert_eq!(day(86_400 * 3 + 5), 3);
    }
}
