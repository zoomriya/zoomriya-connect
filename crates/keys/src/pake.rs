//! Pairing a laptop with the short code: SPAKE2, a password-authenticated
//! key exchange, so that each side proves it knows the code without
//! sending it.
//!
//! The laptop does not know the box key beforehand (a QR code would carry
//! it; a typed code cannot), so the box must prove itself: a fake box on
//! the home network learns nothing it can use, and each guess costs one of
//! the box's few tries.
//!
//! | Step | From | What |
//! |---|---|---|
//! | 1 | Laptop | Its SPAKE2 message |
//! | 2 | Box | Its SPAKE2 message, and its confirmation over the [`transcript`] |
//! | 3 | Laptop | Checks the box's confirmation, then sends its device key, signature and name with its own confirmation |
//!
//! The transcript names the box key and the laptop's link key, as each side
//! sees them on the iroh connection, so a confirmation made for one
//! connection never fits another.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::{KeyError, PublicKey};

const IDENTITY: &[u8] = b"zoomriya laptop pairing v1";

/// Who made a confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The box.
    Box,
    /// The laptop.
    Laptop,
}

impl Side {
    fn label(self) -> &'static [u8] {
        match self {
            Side::Box => b"box\0",
            Side::Laptop => b"laptop\0",
        }
    }
}

/// One side's start of the exchange.
pub struct Start {
    state: Spake2<Ed25519Group>,
    /// The message to send to the other side.
    pub msg: Vec<u8>,
}

impl std::fmt::Debug for Start {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("pake::Start")
    }
}

/// Starts the exchange with the code, as typed and normalised.
pub fn start(code: &str) -> Start {
    let (state, msg) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_bytes()),
        &Identity::new(IDENTITY),
    );
    Start { state, msg }
}

/// The shared key, once both messages are in. It matches the other side's
/// only if both used the same code; the confirmations tell.
pub struct Session([u8; 32]);

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("pake::Session")
    }
}

impl Start {
    /// Takes the other side's message.
    pub fn finish(self, theirs: &[u8]) -> Result<Session, KeyError> {
        let key = self.state.finish(theirs).map_err(|_| KeyError::Malformed)?;
        let key: [u8; 32] = key.try_into().map_err(|_| KeyError::Malformed)?;
        Ok(Session(key))
    }
}

/// What both sides confirm: the box key, the laptop's link key, and both
/// messages, laptop's first. `extra` carries what the laptop sends with its
/// confirmation (its device key, signature and name), empty for the box's.
pub fn transcript(
    box_key: &PublicKey,
    link: &PublicKey,
    laptop_msg: &[u8],
    box_msg: &[u8],
) -> Vec<u8> {
    let mut t = b"zoomriya-laptop-pair/v1\0".to_vec();
    t.extend_from_slice(box_key.as_bytes());
    t.extend_from_slice(link.as_bytes());
    for m in [laptop_msg, box_msg] {
        t.extend_from_slice(&(m.len() as u32).to_be_bytes());
        t.extend_from_slice(m);
    }
    t
}

impl Session {
    fn mac(&self, side: Side, transcript: &[u8], extra: &[u8]) -> Hmac<Sha256> {
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(&self.0)
            .unwrap_or_else(|_| unreachable!("HMAC takes any key length"));
        m.update(side.label());
        m.update(transcript);
        m.update(&(extra.len() as u32).to_be_bytes());
        m.update(extra);
        m
    }

    /// This side's confirmation.
    pub fn confirm(&self, side: Side, transcript: &[u8], extra: &[u8]) -> Vec<u8> {
        self.mac(side, transcript, extra)
            .finalize()
            .into_bytes()
            .to_vec()
    }

    /// Checks the other side's confirmation, in constant time.
    pub fn check(
        &self,
        side: Side,
        transcript: &[u8],
        extra: &[u8],
        theirs: &[u8],
    ) -> Result<(), KeyError> {
        self.mac(side, transcript, extra)
            .verify_slice(theirs)
            .map_err(|_| KeyError::BadSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretKey;

    fn run(laptop_code: &str, box_code: &str) -> (Session, Session, Vec<u8>) {
        let (b, l) = (
            SecretKey::generate().public(),
            SecretKey::generate().public(),
        );
        let lap = start(laptop_code);
        let bx = start(box_code);
        let t = transcript(&b, &l, &lap.msg, &bx.msg);
        let (lm, bm) = (lap.msg.clone(), bx.msg.clone());
        (
            lap.finish(&bm).expect("laptop"),
            bx.finish(&lm).expect("box"),
            t,
        )
    }

    #[test]
    fn same_code_confirms() {
        let (lap, bx, t) = run("4K7P2QXM", "4K7P2QXM");
        let c = bx.confirm(Side::Box, &t, b"");
        assert!(lap.check(Side::Box, &t, b"", &c).is_ok());
        let c = lap.confirm(Side::Laptop, &t, b"device");
        assert!(bx.check(Side::Laptop, &t, b"device", &c).is_ok());
        // Not for the other side, other extras or another transcript.
        assert!(bx.check(Side::Box, &t, b"device", &c).is_err());
        assert!(bx.check(Side::Laptop, &t, b"other", &c).is_err());
        assert!(bx.check(Side::Laptop, b"other", b"device", &c).is_err());
    }

    #[test]
    fn another_code_does_not() {
        let (lap, bx, t) = run("4K7P2QXM", "4K7P2QXN");
        let c = bx.confirm(Side::Box, &t, b"");
        assert_eq!(
            lap.check(Side::Box, &t, b"", &c),
            Err(KeyError::BadSignature)
        );
    }
}
