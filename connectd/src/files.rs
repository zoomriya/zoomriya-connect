//! Files only connectd may read: keys and the device list.

use std::path::Path;

use anyhow::{Context, Result};
use zoomriya_keys::SecretKey;

/// Writes a file readable by its owner only, whole or not at all.
pub fn write_private(file: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = file.parent()
        && !dir.as_os_str().is_empty()
    {
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
    std::fs::rename(&tmp, file).with_context(|| format!("writing {}", file.display()))?;
    Ok(())
}

/// Reads the secret behind the box's DHT record, or makes one the first time.
pub fn record_secret(file: &Path) -> Result<zoomriya_keys::record::RecordSecret> {
    use zoomriya_keys::record::RecordSecret;
    match std::fs::read(file) {
        Ok(b) => {
            let bytes: [u8; 32] = b
                .as_slice()
                .try_into()
                .with_context(|| format!("{} isn't a record secret", file.display()))?;
            Ok(RecordSecret::from_bytes(bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let s = RecordSecret::generate().map_err(|e| anyhow::anyhow!("{e}"))?;
            write_private(file, &s.to_bytes())?;
            Ok(s)
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

/// Reads an Ed25519 key, or makes one the first time.
pub fn key(file: &Path) -> Result<SecretKey> {
    match std::fs::read(file) {
        Ok(b) => {
            let bytes: [u8; 32] = b
                .as_slice()
                .try_into()
                .with_context(|| format!("{} isn't a key", file.display()))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let k = SecretKey::generate();
            write_private(file, &k.to_bytes())?;
            Ok(k)
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_stay_the_same() {
        let dir = tempfile::tempdir().expect("dir");
        let f = dir.path().join("k/box.key");
        let a = key(&f).expect("made");
        let b = key(&f).expect("read");
        assert_eq!(a.public(), b.public());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&f).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(&f, b"short").expect("write");
        assert!(key(&f).is_err());
    }
}
