//! What connectd remembers besides keys and devices: its server, its name,
//! and the family's choices in Box Desktop.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The saved state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// The ZOOMRIYA Connect server, if Box Desktop set one other than the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// The box's name, once registered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its full name, such as `maplehouse.zoomriya.app`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fqdn: Option<String>,
    /// The guests' door's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_fqdn: Option<String>,
    /// The ZOOMRIYA Connect account the box is linked to (its username only:
    /// the password is never kept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// ZOOMRIYA Connect switched off in Box Desktop.
    #[serde(default)]
    pub off: bool,
    /// The relay switched off: the address record then holds the box's own
    /// addresses, and Box Desktop says so.
    #[serde(default)]
    pub relay_off: bool,
    /// When the removed-device list last changed, so relays take it in order.
    #[serde(default)]
    pub removed_issued: u64,
}

/// The state, in a file.
#[derive(Debug)]
pub struct Saved {
    file: PathBuf,
    state: std::sync::Mutex<State>,
}

impl Saved {
    /// Reads `file`, or starts afresh.
    pub fn open(file: &Path) -> Result<Self> {
        let state = match std::fs::read(file) {
            Ok(b) => {
                serde_json::from_slice(&b).with_context(|| format!("reading {}", file.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            file: file.to_path_buf(),
            state: std::sync::Mutex::new(state),
        })
    }

    /// A copy of the state.
    pub fn get(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Changes the state and saves it.
    pub fn update<T>(&self, f: impl FnOnce(&mut State) -> T) -> Result<T> {
        let mut s = self.state.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let out = f(&mut s);
        crate::files::write_private(&self.file, &serde_json::to_vec_pretty(&*s)?)?;
        Ok(out)
    }
}
