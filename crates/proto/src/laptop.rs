//! Pairing a laptop: Box Desktop shows a short code, which the person types
//! into the laptop app at home. Short enough to type, so it allows only
//! [`CODE_TRIES`] tries, and it is never sent: both sides prove they know it
//! with SPAKE2 ([`zoomriya_keys::pake`]), on the pairing stream:
//!
//! | Step | From | Message |
//! |---|---|---|
//! | 1 | Laptop | [`LaptopHello`] |
//! | 2 | Box | [`crate::link::PairResponse::Challenge`], or `Refused` |
//! | 3 | Laptop | [`LaptopFinish`] |
//! | 4 | Box | [`crate::link::PairResponse::Paired`], or `Refused` |

use serde::{Deserialize, Serialize};

/// A pairing code: tries before it's used up.
pub const CODE_TRIES: u32 = 3;

/// Step 1, laptop to box: its SPAKE2 message ([`zoomriya_keys::pake`]).
/// The code itself is never sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaptopHello {
    /// The laptop's SPAKE2 message, base64url.
    pub spake: String,
}

/// Step 3, laptop to box, once the box's confirmation checked out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaptopFinish {
    /// The laptop's device key: P-256, SEC1, base64url. Made in the laptop's
    /// TPM or keychain where it has one.
    pub device_key: String,
    /// The device key's signature over the link key (the connection's own
    /// iroh key), base64url.
    pub link_sig: String,
    /// What the laptop calls itself, such as "Anna's laptop".
    pub name: String,
    /// The laptop's confirmation over the transcript and [`Self::extra`],
    /// base64url.
    pub confirm: String,
}

impl LaptopFinish {
    /// What the laptop's confirmation covers besides the transcript.
    pub fn extra(&self) -> Vec<u8> {
        let mut e = Vec::new();
        for f in [&self.device_key, &self.link_sig, &self.name] {
            e.extend_from_slice(&(f.len() as u32).to_be_bytes());
            e.extend_from_slice(f.as_bytes());
        }
        e
    }
}

/// A pairing code for a laptop: 8 letters and digits, shown as `4K7P-2QXM`,
/// without the ones that look alike.
pub fn new_code(random: [u8; 8]) -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    let c: String = random
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect();
    format!("{}-{}", &c[..4], &c[4..])
}

/// A typed code as compared: capitals, without the dash or spaces.
pub fn normalise_code(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        let c = new_code([0, 1, 2, 3, 250, 251, 252, 253]);
        assert_eq!(c.len(), 9);
        assert_eq!(&c[4..5], "-");
        assert!(!c.contains(['0', '1', 'I', 'L', 'O']));
        assert_eq!(normalise_code(" 4k7p-2qxm "), "4K7P2QXM");
        assert_eq!(normalise_code(&c), c.replace('-', ""));
    }
}
