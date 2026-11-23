//! Remote access away from home: on unless the family turns it off in Box
//! Desktop (Control Panel › Remote access). Kept in `away.json`.
//!
//! | Off means | |
//! |---|---|
//! | The DHT record | No longer published; the last one fades within a day |
//! | Connections | Only over the home network; others are closed at once |
//! | The box's relay | Lets in the box only |
//!
//! Pairing and use at home go on as before.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct File {
    on: bool,
}

/// Whether paired devices may reach the box away from home. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Away {
    on: Arc<AtomicBool>,
    file: Option<PathBuf>,
}

impl Default for Away {
    /// On, kept nowhere: tests.
    fn default() -> Self {
        Self {
            on: Arc::new(AtomicBool::new(true)),
            file: None,
        }
    }
}

impl Away {
    /// Reads `file`; a missing file means on.
    pub fn open(file: &Path) -> Result<Self> {
        let on = match std::fs::read(file) {
            Ok(b) => {
                serde_json::from_slice::<File>(&b)
                    .with_context(|| format!("reading {}", file.display()))?
                    .on
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            on: Arc::new(AtomicBool::new(on)),
            file: Some(file.to_path_buf()),
        })
    }

    /// Whether it is on now.
    pub fn on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Turns it on or off, and keeps the choice.
    pub fn set(&self, on: bool) -> Result<()> {
        if let Some(f) = &self.file {
            crate::files::write_private(f, &serde_json::to_vec(&File { on })?)?;
        }
        self.on.store(on, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_until_turned_off_and_kept() {
        let dir = tempfile::tempdir().expect("dir");
        let f = dir.path().join("away.json");
        let a = Away::open(&f).expect("open");
        assert!(a.on());
        a.set(false).expect("set");
        assert!(!a.clone().on());
        assert!(!Away::open(&f).expect("again").on());
    }
}
