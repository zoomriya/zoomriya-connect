//! Whether the box's clock was set from the internet. Passes are signed only
//! then (docs/architecture.md, "Versions and updates").

use std::sync::atomic::{AtomicBool, Ordering};

static TRUST: AtomicBool = AtomicBool::new(false);

/// systemd-timesyncd makes this file once the clock is set.
const SYNCED: &str = "/run/systemd/timesync/synchronized";

/// Whether the clock can be trusted.
pub fn synced() -> bool {
    TRUST.load(Ordering::Relaxed) || std::path::Path::new(SYNCED).exists()
}

/// For a machine without systemd-timesyncd, set from the config.
pub fn trust_clock() {
    TRUST.store(true, Ordering::Relaxed);
}

#[cfg(test)]
pub fn assume_synced_for_tests() {
    trust_clock();
}
