//! Keeping the list of boxes: from the control plane, or from boxes that
//! enrolled with a one-time code. Saved to disk, so a relay that restarts
//! while the control plane is down still knows every box.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zoomriya_keys::Key;
use zoomriya_proto::{
    BoxChange, BoxConnected, BoxFeed, BoxUse, PROTOCOL_VERSION, RelayReport, RelayReportAnswer,
    VERSION_HEADER,
};

use crate::gate::Gate;

/// What's saved: where the feed got to, and the boxes.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Saved {
    /// The control plane's sequence number we've read up to.
    pub seq: u64,
    /// Every box we know.
    pub boxes: Vec<BoxChange>,
}

/// The list's file in the state directory.
pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("boxes.json")
}

/// Reads the saved list, if there is one.
pub fn load(file: &Path) -> Result<Saved> {
    match std::fs::read(file) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).with_context(|| format!("reading {}", file.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Saved::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

/// Saves the gate's boxes, writing a new file and moving it into place so a
/// crash never leaves half a file.
pub fn save(file: &Path, seq: u64, gate: &Gate) -> Result<()> {
    let mut boxes: Vec<BoxChange> = gate
        .boxes()
        .into_iter()
        .map(|(k, state)| BoxChange { key: Key(k), state })
        .collect();
    boxes.sort_by_key(|c| c.key);
    let bytes = serde_json::to_vec_pretty(&Saved { seq, boxes })?;
    let tmp = file.with_extension("json.new");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, file).with_context(|| format!("writing {}", file.display()))?;
    Ok(())
}

/// Loads the saved list into the gate; returns the sequence number.
pub fn restore(file: &Path, gate: &Gate) -> Result<u64> {
    let saved = load(file)?;
    gate.replace_boxes(saved.boxes.iter().map(|c| (c.key.0, c.state)));
    info!(boxes = saved.boxes.len(), "boxes from disk");
    Ok(saved.seq)
}

/// Asks the control plane what changed, and reports each box's use and
/// who's connected, forever.
pub async fn follow(
    gate: Arc<Gate>,
    url: String,
    key: String,
    name: String,
    every: Duration,
    file: PathBuf,
    mut seq: u64,
) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("zoomriya-relay/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            warn!("can't make an HTTP client: {err}");
            return;
        }
    };
    let base = url.trim_end_matches('/').to_string();
    let mut failures = 0u32;
    loop {
        match fetch(&client, &base, &key, seq).await {
            Ok(feed) => {
                failures = 0;
                let changed = feed.seq != seq || !feed.changes.is_empty();
                if feed.full {
                    // An empty control plane answers with the whole (empty)
                    // list every time: only say so when something changed.
                    let before = gate.boxes().len();
                    gate.replace_boxes(feed.changes.iter().map(|c| (c.key.0, c.state)));
                    if feed.seq != seq || before != feed.changes.len() {
                        info!(
                            boxes = feed.changes.len(),
                            "full list from the control plane"
                        );
                    }
                } else {
                    for c in &feed.changes {
                        gate.apply(c.key.0, c.state);
                    }
                }
                seq = feed.seq;
                if changed && let Err(err) = save(&file, seq, &gate) {
                    warn!("saving boxes: {err:#}");
                }
            }
            Err(err) => {
                failures += 1;
                // Say so once a minute or so, not every poll.
                if failures == 1 || failures.is_multiple_of(6) {
                    warn!("control plane: {err:#} (keeping the list we have)");
                }
            }
        }
        report(&client, &base, &key, &name, &gate).await;
        tokio::time::sleep(every).await;
    }
}

/// Sends what each box used and who's connected; slows whom the answer names.
/// Counts that didn't get through go in the next report.
async fn report(client: &reqwest::Client, base: &str, key: &str, name: &str, gate: &Gate) {
    let used = gate.meter.take();
    let body = RelayReport {
        relay: name.to_string(),
        used: used
            .iter()
            .map(|(k, bytes)| BoxUse {
                key: Key(*k),
                bytes: *bytes,
            })
            .collect(),
        connected: gate
            .connected_boxes()
            .into_iter()
            .map(|(k, devices)| BoxConnected {
                key: Key(k),
                devices,
            })
            .collect(),
    };
    let answer = async {
        client
            .post(format!("{base}/v1/relay/report"))
            .bearer_auth(key)
            .header(VERSION_HEADER, PROTOCOL_VERSION)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<RelayReportAnswer>()
            .await
    };
    match answer.await {
        Ok(a) => {
            let before = gate.meter.slowed();
            gate.meter
                .set_slow(a.slow.into_iter().map(|k| k.0), a.slow_bytes_per_second);
            let after = gate.meter.slowed();
            if before != after {
                info!(boxes = after, "boxes past their allowance, slowed");
            }
        }
        Err(err) => {
            gate.meter.put_back(used);
            tracing::debug!("report: {err:#}");
        }
    }
}

async fn fetch(client: &reqwest::Client, base: &str, key: &str, after: u64) -> Result<BoxFeed> {
    let res = client
        .get(format!("{base}/v1/relay/boxes?after={after}"))
        .bearer_auth(key)
        .header(VERSION_HEADER, PROTOCOL_VERSION)
        .send()
        .await?
        .error_for_status()?;
    Ok(res.json().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::GateConfig;
    use zoomriya_keys::SecretKey;
    use zoomriya_proto::BoxState;

    #[test]
    fn save_and_restore() {
        let dir = tempfile::tempdir().expect("dir");
        let file = path(dir.path());
        let cfg = GateConfig {
            clock_allowance: 600,
            longest_pass: 31 * 86_400,
        };
        let gate = Gate::new(cfg);
        assert_eq!(restore(&file, &gate).expect("empty is fine"), 0);
        let k = SecretKey::generate().public();
        gate.apply(k, BoxState::ActiveFactory);
        save(&file, 42, &gate).expect("saves");
        let again = Gate::new(cfg);
        assert_eq!(restore(&file, &again).expect("reads"), 42);
        assert_eq!(again.boxes(), vec![(k, BoxState::ActiveFactory)]);
    }
}
