//! One-time codes for a family's own relay (docs/sharing-and-names.md, "How
//! your relay knows your box"). `zoomriya-relay enrol-code` writes a code to
//! the state directory and shows it; the family types it into Box Desktop and
//! the box introduces itself. The code works once, for ten minutes, with five
//! tries.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// How long a code works.
pub const CODE_LIFETIME: u64 = 600;
/// Wrong tries before the code stops working.
pub const CODE_TRIES: u32 = 5;
/// Letters and numbers that can't be mistaken for each other.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 10;

#[derive(Debug, Serialize, Deserialize)]
struct Pending {
    sha256: String,
    expires: u64,
    tries_left: u32,
}

/// Why a code was refused. Box Desktop shows one message for all of them, so
/// a guesser learns nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CodeRefused {
    /// No code waiting, used up, expired, or wrong.
    #[error("That code doesn't work. Make a new one on your relay and try again.")]
    NoGood,
}

fn file(state_dir: &Path) -> PathBuf {
    state_dir.join("enrol-code.json")
}

fn hash(code: &str) -> String {
    let norm: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    HEXLOWER.encode(&Sha256::digest(norm.as_bytes()))
}

/// Makes a new code, replacing any waiting one, and returns it for showing,
/// as two groups of five.
pub fn new_code(state_dir: &Path, now: u64) -> Result<String> {
    let mut bytes = [0u8; CODE_LEN];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
    // 31 letters: the slight bias of `% 31` costs well under a bit.
    let code: String = bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect();
    let pending = Pending {
        sha256: hash(&code),
        expires: now + CODE_LIFETIME,
        tries_left: CODE_TRIES,
    };
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("making {}", state_dir.display()))?;
    let path = file(state_dir);
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, serde_json::to_vec(&pending)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(format!("{}-{}", &code[..5], &code[5..]))
}

/// Checks a code a box sent. Right: the code is used up. Wrong: one try less.
/// The caller holds a lock, so two tries never race.
pub fn redeem(state_dir: &Path, code: &str, now: u64) -> Result<(), CodeRefused> {
    let path = file(state_dir);
    let Ok(bytes) = std::fs::read(&path) else {
        return Err(CodeRefused::NoGood);
    };
    let Ok(mut pending) = serde_json::from_slice::<Pending>(&bytes) else {
        let _ = std::fs::remove_file(&path);
        return Err(CodeRefused::NoGood);
    };
    if now > pending.expires || pending.tries_left == 0 {
        let _ = std::fs::remove_file(&path);
        return Err(CodeRefused::NoGood);
    }
    let right: bool = hash(code)
        .as_bytes()
        .ct_eq(pending.sha256.as_bytes())
        .into();
    if right {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    pending.tries_left -= 1;
    if pending.tries_left == 0 {
        let _ = std::fs::remove_file(&path);
    } else if let Ok(b) = serde_json::to_vec(&pending) {
        let _ = std::fs::write(&path, b);
    }
    Err(CodeRefused::NoGood)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_once() {
        let dir = tempfile::tempdir().expect("dir");
        let code = new_code(dir.path(), 1000).expect("code");
        assert_eq!(code.len(), 11);
        // Typed in lower case, without the hyphen.
        let typed = code.replace('-', "").to_lowercase();
        assert_eq!(redeem(dir.path(), &typed, 1100), Ok(()));
        assert_eq!(redeem(dir.path(), &code, 1100), Err(CodeRefused::NoGood));
    }

    #[test]
    fn runs_out() {
        let dir = tempfile::tempdir().expect("dir");
        let code = new_code(dir.path(), 1000).expect("code");
        assert_eq!(
            redeem(dir.path(), &code, 1000 + CODE_LIFETIME + 1),
            Err(CodeRefused::NoGood)
        );
        let code = new_code(dir.path(), 1000).expect("code");
        for _ in 0..CODE_TRIES {
            assert_eq!(
                redeem(dir.path(), "WRONG-CODE", 1001),
                Err(CodeRefused::NoGood)
            );
        }
        assert_eq!(redeem(dir.path(), &code, 1002), Err(CodeRefused::NoGood));
    }
}
