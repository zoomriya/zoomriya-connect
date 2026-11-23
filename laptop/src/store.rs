//! What the laptop app keeps: its keys, the paired box, and the secret its
//! browser shows. In the user's own app data folder, readable by the user
//! only.
//!
//! | File | What |
//! |---|---|
//! | `link.key` | The link key: the laptop's iroh key (Ed25519) |
//! | `device.key` | The device key (P-256), until the keychain or TPM holds it |
//! | `box.json` | The paired box: its key, its addresses, the secret behind its DHT record, its onion address |
//! | `browser.secret` | The session cookie's value |
//! | `tor/`, `tor-cache/` | Tor's state and what it downloads, when the box has an onion address |

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use data_encoding::BASE64URL_NOPAD;
use serde::{Deserialize, Serialize};
use zoomriya_keys::{Key, SecretKey};
use zoomriya_link::device::SoftKey;

/// The paired box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedBox {
    /// Its key, pinned at pairing.
    pub key: Key,
    /// Where it was last reached.
    pub addrs: Vec<SocketAddr>,
    /// The secret behind its record in the DHT, base64url: how the laptop
    /// finds it from away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    /// Its onion address: its own relay through Tor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onion: Option<String>,
}

/// The app's folder.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

/// The usual folder for the app's data on this system.
pub fn default_dir() -> Result<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/ZOOMRIYA"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("ZOOMRIYA"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
            .map(|d| d.join("zoomriya"))
    };
    dir.context("no home folder")
}

fn write_private(file: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }
    let tmp = file.with_extension("new");
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp)
            .with_context(|| format!("writing {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, file).with_context(|| format!("writing {}", file.display()))
}

fn random(n: usize) -> Result<Vec<u8>> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
    Ok(b)
}

impl Store {
    /// The folder `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn read_or_make(&self, name: &str, make: impl FnOnce() -> Result<Vec<u8>>) -> Result<Vec<u8>> {
        let f = self.dir.join(name);
        match std::fs::read(&f) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let b = make()?;
                write_private(&f, &b)?;
                Ok(b)
            }
            Err(e) => Err(e).with_context(|| format!("reading {}", f.display())),
        }
    }

    /// Tor's folders, state and cache, readable by the user only.
    pub fn tor_dirs(&self) -> Result<(PathBuf, PathBuf)> {
        let dirs = (self.dir.join("tor"), self.dir.join("tor-cache"));
        for d in [&dirs.0, &dirs.1] {
            std::fs::create_dir_all(d).with_context(|| format!("making {}", d.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))?;
                std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(dirs)
    }

    /// The link key, made the first time.
    pub fn link_key(&self) -> Result<SecretKey> {
        let b = self.read_or_make("link.key", || Ok(SecretKey::generate().to_bytes().to_vec()))?;
        let b: [u8; 32] = b.as_slice().try_into().context("link.key isn't a key")?;
        Ok(SecretKey::from_bytes(&b))
    }

    /// The device key, made the first time.
    pub fn device_key(&self) -> Result<SoftKey> {
        let b = self.read_or_make("device.key", || Ok(SoftKey::generate()?.to_bytes()))?;
        SoftKey::from_bytes(&b)
    }

    /// The secret the browser's session cookie holds, made the first time,
    /// so the browser stays signed in to the app across restarts.
    pub fn browser_secret(&self) -> Result<String> {
        let b = self.read_or_make("browser.secret", || {
            Ok(BASE64URL_NOPAD.encode(&random(32)?).into_bytes())
        })?;
        Ok(String::from_utf8(b)?.trim().to_string())
    }

    /// The paired box, if any.
    pub fn paired(&self) -> Result<Option<PairedBox>> {
        match std::fs::read(self.dir.join("box.json")) {
            Ok(b) => Ok(Some(
                serde_json::from_slice(&b).context("reading box.json")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Saves the paired box.
    pub fn save_paired(&self, b: &PairedBox) -> Result<()> {
        write_private(&self.dir.join("box.json"), &serde_json::to_vec_pretty(b)?)
    }

    /// Forgets the box, and the link key with it, so pairing again starts
    /// afresh.
    pub fn forget(&self) -> Result<()> {
        for f in ["box.json", "link.key"] {
            match std::fs::remove_file(self.dir.join(f)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_stay_and_files_are_private() {
        let dir = tempfile::tempdir().expect("dir");
        let s = Store::new(dir.path().join("z"));
        let a = s.link_key().expect("made");
        assert_eq!(a.public(), s.link_key().expect("read").public());
        assert_eq!(
            s.browser_secret().expect("made"),
            s.browser_secret().expect("read")
        );
        s.device_key().expect("device");
        assert_eq!(s.paired().expect("none"), None);
        let b = PairedBox {
            key: Key(SecretKey::generate().public()),
            addrs: vec!["192.168.1.20:443".parse().expect("addr")],
            record: Some("c2VjcmV0".into()),
            onion: None,
        };
        s.save_paired(&b).expect("saved");
        assert_eq!(s.paired().expect("read"), Some(b));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in ["link.key", "device.key", "box.json", "browser.secret"] {
                let m = std::fs::metadata(dir.path().join("z").join(f)).expect("meta");
                assert_eq!(m.permissions().mode() & 0o777, 0o600, "{f}");
            }
        }
        s.forget().expect("forgot");
        assert_eq!(s.paired().expect("none"), None);
        assert_ne!(a.public(), s.link_key().expect("new").public());
    }
}
