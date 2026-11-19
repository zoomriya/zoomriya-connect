//! Who may use the relay: registered boxes, and devices with a pass from one.
//!
//! This is our check inside iroh's relay library (its `AccessControl` hook),
//! which sees the key each client proved and the pass it sent as its token.
//! The relay never asks anyone about devices: the pass says everything.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock, RwLock},
};

use iroh_metrics::{Counter, MetricsGroup};
use iroh_relay::server::{Access, AccessControl, ClientRequest, ConnectionId, clients::Clients};
use tracing::{debug, info};
use zoomriya_keys::{Pass, PassRefused, PublicKey};
use zoomriya_proto::{BoxState, RemovedDevices};

/// Most removed devices kept per box. A family removes a handful a year.
pub const MAX_REMOVED_PER_BOX: usize = 1000;

/// Counts of what the gate let in and turned away. Never per device.
#[derive(Debug, Default, MetricsGroup)]
#[metrics(name = "zoomriya_gate")]
pub struct GateMetrics {
    /// Boxes let in.
    pub boxes_allowed: Counter,
    /// Devices let in with a pass.
    pub devices_allowed: Counter,
    /// Refused: no pass.
    pub refused_missing: Counter,
    /// Refused: not a pass, or damaged.
    pub refused_malformed: Counter,
    /// Refused: not signed by its box.
    pub refused_bad_signature: Counter,
    /// Refused: the pass is for another device.
    pub refused_wrong_device: Counter,
    /// Refused: not valid yet.
    pub refused_not_yet_valid: Counter,
    /// Refused: ran out.
    pub refused_expired: Counter,
    /// Refused: lasts longer than allowed.
    pub refused_too_long: Counter,
    /// Refused: the box isn't registered here.
    pub refused_unknown_box: Counter,
    /// Refused: the box removed the device.
    pub refused_removed: Counter,
    /// Refused: the box was marked stolen.
    pub refused_stolen: Counter,
    /// Removed-device lists taken from boxes.
    pub removed_lists: Counter,
    /// Live connections dropped because of a removal or a stolen box.
    pub dropped: Counter,
}

impl GateMetrics {
    fn refused(&self, why: PassRefused) {
        let c = match why {
            PassRefused::Missing => &self.refused_missing,
            PassRefused::Malformed => &self.refused_malformed,
            PassRefused::BadSignature => &self.refused_bad_signature,
            PassRefused::WrongDevice => &self.refused_wrong_device,
            PassRefused::NotYetValid => &self.refused_not_yet_valid,
            PassRefused::Expired => &self.refused_expired,
            PassRefused::TooLong => &self.refused_too_long,
            PassRefused::UnknownBox => &self.refused_unknown_box,
            PassRefused::Removed => &self.refused_removed,
            PassRefused::Stolen => &self.refused_stolen,
        };
        c.inc();
    }
}

/// The gate's limits.
#[derive(Debug, Clone, Copy)]
pub struct GateConfig {
    /// Clock difference allowed, in seconds (10 minutes in the design).
    pub clock_allowance: u64,
    /// Longest pass accepted, in seconds.
    pub longest_pass: u64,
}

#[derive(Debug, Default)]
struct Removed {
    /// When the box made the last list we took.
    issued: u64,
    /// Link key → forget after (seconds since 1970).
    devices: HashMap<PublicKey, u64>,
}

#[derive(Debug)]
struct Live {
    /// The box a device came in for, or the box itself.
    box_key: PublicKey,
    conns: HashSet<ConnectionId>,
}

/// Why a removed-device list was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ListRefused {
    /// The box isn't registered here.
    #[error("unknown box")]
    UnknownBox,
    /// Older than, or the same as, the last list from this box.
    #[error("not newer than the last list")]
    Stale,
    /// Made too far from now.
    #[error("clock too far off")]
    Clock,
    /// More devices than we keep.
    #[error("too many devices")]
    TooMany,
}

/// The relay's gate. Shared by the relay service and the API.
#[derive(Debug)]
pub struct Gate {
    cfg: GateConfig,
    boxes: RwLock<HashMap<PublicKey, BoxState>>,
    removed: Mutex<HashMap<PublicKey, Removed>>,
    live: Mutex<HashMap<PublicKey, Live>>,
    clients: OnceLock<Clients>,
    now: fn() -> u64,
    /// What the gate let in and turned away.
    pub metrics: Arc<GateMetrics>,
}

impl Gate {
    /// A gate with no boxes yet.
    pub fn new(cfg: GateConfig) -> Self {
        Self::with_clock(cfg, zoomriya_serve::now)
    }

    fn with_clock(cfg: GateConfig, now: fn() -> u64) -> Self {
        Self {
            cfg,
            boxes: RwLock::default(),
            removed: Mutex::default(),
            live: Mutex::default(),
            clients: OnceLock::new(),
            now,
            metrics: Arc::default(),
        }
    }

    /// Hands the gate the relay's connected clients, so it can drop removed
    /// devices at once.
    pub fn set_clients(&self, clients: Clients) {
        let _ = self.clients.set(clients);
    }

    /// Replaces the whole list of boxes.
    pub fn replace_boxes(&self, boxes: impl IntoIterator<Item = (PublicKey, BoxState)>) {
        let new: HashMap<_, _> = boxes
            .into_iter()
            .filter(|(_, s)| *s != BoxState::Gone)
            .collect();
        let dropped: Vec<PublicKey> = {
            let Ok(mut cur) = self.boxes.write() else {
                return;
            };
            let dropped = cur
                .iter()
                .filter(|(k, _)| !matches!(new.get(*k), Some(s) if is_active(*s)))
                .map(|(k, _)| *k)
                .collect();
            *cur = new;
            dropped
        };
        for b in dropped {
            self.drop_box(&b);
        }
    }

    /// Applies one change to the list of boxes.
    pub fn apply(&self, key: PublicKey, state: BoxState) {
        {
            let Ok(mut cur) = self.boxes.write() else {
                return;
            };
            if state == BoxState::Gone {
                cur.remove(&key);
            } else {
                cur.insert(key, state);
            }
        }
        if !is_active(state) {
            self.drop_box(&key);
        }
        if state == BoxState::Gone
            && let Ok(mut r) = self.removed.lock()
        {
            r.remove(&key);
        }
    }

    /// The boxes, for saving.
    pub fn boxes(&self) -> Vec<(PublicKey, BoxState)> {
        self.boxes
            .read()
            .map(|b| b.iter().map(|(k, s)| (*k, *s)).collect())
            .unwrap_or_default()
    }

    /// Connected endpoints: boxes and devices.
    pub fn live(&self) -> (usize, usize) {
        self.live
            .lock()
            .map(|l| {
                let boxes = l.iter().filter(|(k, v)| *k == &v.box_key).count();
                (boxes, l.len() - boxes)
            })
            .unwrap_or((0, 0))
    }

    fn box_state(&self, key: &PublicKey) -> Option<BoxState> {
        self.boxes.read().ok().and_then(|b| b.get(key).copied())
    }

    /// Takes a box's list of removed devices (already checked to be signed by
    /// that box), and drops any of them connected now.
    pub fn take_removed(&self, list: &RemovedDevices) -> Result<usize, ListRefused> {
        let box_key = list.box_key.0;
        if !self.box_state(&box_key).is_some_and(is_active) {
            return Err(ListRefused::UnknownBox);
        }
        let now = (self.now)();
        if list.issued.abs_diff(now) > self.cfg.clock_allowance {
            return Err(ListRefused::Clock);
        }
        if list.devices.len() > MAX_REMOVED_PER_BOX {
            return Err(ListRefused::TooMany);
        }
        let forget_before = now.saturating_sub(self.cfg.clock_allowance);
        let to_drop: Vec<PublicKey> = {
            let Ok(mut all) = self.removed.lock() else {
                return Err(ListRefused::Stale);
            };
            let entry = all.entry(box_key).or_default();
            if list.issued <= entry.issued {
                return Err(ListRefused::Stale);
            }
            entry.issued = list.issued;
            if list.full {
                entry.devices.clear();
            }
            for d in &list.devices {
                entry.devices.insert(d.link.0, d.until);
            }
            entry.devices.retain(|_, until| *until >= forget_before);
            if entry.devices.len() > MAX_REMOVED_PER_BOX {
                return Err(ListRefused::TooMany);
            }
            list.devices.iter().map(|d| d.link.0).collect()
        };
        self.metrics.removed_lists.inc();
        let mut dropped = 0;
        for link in to_drop {
            if self.drop_if_for(&link, &box_key) {
                dropped += 1;
            }
        }
        Ok(dropped)
    }

    fn is_removed(&self, box_key: &PublicKey, link: &PublicKey) -> bool {
        let now = (self.now)();
        self.removed.lock().is_ok_and(|r| {
            r.get(box_key)
                .and_then(|e| e.devices.get(link))
                .is_some_and(|until| until.saturating_add(self.cfg.clock_allowance) >= now)
        })
    }

    /// The whole check, without the relay around it.
    pub fn decide(
        &self,
        endpoint: &PublicKey,
        token: Option<&str>,
    ) -> Result<PublicKey, PassRefused> {
        match self.box_state(endpoint) {
            Some(s) if is_active(s) => return Ok(*endpoint),
            Some(BoxState::Stolen) => return Err(PassRefused::Stolen),
            _ => {}
        }
        let token = token.ok_or(PassRefused::Missing)?;
        let pass = Pass::open(token)?;
        pass.check(
            endpoint,
            (self.now)(),
            self.cfg.clock_allowance,
            self.cfg.longest_pass,
        )?;
        match self.box_state(&pass.box_key) {
            Some(s) if is_active(s) => {}
            Some(BoxState::Stolen) => return Err(PassRefused::Stolen),
            _ => return Err(PassRefused::UnknownBox),
        }
        if self.is_removed(&pass.box_key, endpoint) {
            return Err(PassRefused::Removed);
        }
        Ok(pass.box_key)
    }

    fn drop_box(&self, box_key: &PublicKey) {
        let victims: Vec<PublicKey> = self
            .live
            .lock()
            .map(|l| {
                l.iter()
                    .filter(|(_, v)| &v.box_key == box_key)
                    .map(|(k, _)| *k)
                    .collect()
            })
            .unwrap_or_default();
        for v in victims {
            self.drop_if_for(&v, box_key);
        }
    }

    fn drop_if_for(&self, endpoint: &PublicKey, box_key: &PublicKey) -> bool {
        let is_for = self
            .live
            .lock()
            .is_ok_and(|l| l.get(endpoint).is_some_and(|v| &v.box_key == box_key));
        if !is_for {
            return false;
        }
        if let Some(clients) = self.clients.get() {
            clients.disconnect(*endpoint, None);
        }
        self.metrics.dropped.inc();
        info!(endpoint = %endpoint.fmt_short(), "dropped");
        true
    }

    fn connected(&self, endpoint: PublicKey, box_key: PublicKey, conn: ConnectionId) {
        if let Ok(mut l) = self.live.lock() {
            let e = l.entry(endpoint).or_insert_with(|| Live {
                box_key,
                conns: HashSet::new(),
            });
            e.box_key = box_key;
            e.conns.insert(conn);
        }
    }
}

fn is_active(s: BoxState) -> bool {
    matches!(s, BoxState::Active | BoxState::ActiveFactory)
}

/// The gate, as iroh's relay library calls it.
#[derive(Debug, Clone)]
pub struct GateAccess(pub Arc<Gate>);

impl AccessControl for GateAccess {
    async fn on_connect(&self, request: &ClientRequest) -> Access {
        let endpoint = request.endpoint_id();
        let token = request.auth_token();
        match self.0.decide(&endpoint, token.as_deref()) {
            Ok(box_key) => {
                if box_key == endpoint {
                    self.0.metrics.boxes_allowed.inc();
                } else {
                    self.0.metrics.devices_allowed.inc();
                }
                self.0.connected(endpoint, box_key, request.connection_id());
                Access::Allow
            }
            Err(why) => {
                self.0.metrics.refused(why);
                debug!(endpoint = %endpoint.fmt_short(), %why, "refused");
                Access::Deny {
                    reason: Some(why.to_string()),
                }
            }
        }
    }

    fn on_disconnect(&self, endpoint: PublicKey, connection_id: ConnectionId) {
        if let Ok(mut l) = self.0.live.lock()
            && let Some(e) = l.get_mut(&endpoint)
        {
            e.conns.remove(&connection_id);
            if e.conns.is_empty() {
                l.remove(&endpoint);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoomriya_keys::SecretKey;
    use zoomriya_proto::RemovedDevice;

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn gate() -> Gate {
        Gate::with_clock(
            GateConfig {
                clock_allowance: 600,
                longest_pass: 31 * DAY,
            },
            || NOW,
        )
    }

    fn token(b: &SecretKey, link: &PublicKey) -> String {
        Pass {
            box_key: b.public(),
            link_key: *link,
            not_before: NOW - DAY,
            not_after: NOW + 29 * DAY,
        }
        .sign(b)
        .expect("signs")
    }

    #[test]
    fn boxes_and_their_devices() {
        let g = gate();
        let b = SecretKey::generate();
        let phone = SecretKey::generate().public();
        let t = token(&b, &phone);

        // Not registered yet.
        assert_eq!(g.decide(&b.public(), None), Err(PassRefused::Missing));
        assert_eq!(g.decide(&phone, Some(&t)), Err(PassRefused::UnknownBox));

        g.apply(b.public(), BoxState::Active);
        assert_eq!(g.decide(&b.public(), None), Ok(b.public()));
        assert_eq!(g.decide(&phone, Some(&t)), Ok(b.public()));
        assert_eq!(g.decide(&phone, None), Err(PassRefused::Missing));

        // Someone else showing the phone's pass.
        let thief = SecretKey::generate().public();
        assert_eq!(g.decide(&thief, Some(&t)), Err(PassRefused::WrongDevice));

        // A pass from a box that isn't registered, for a registered box's key.
        let rogue = SecretKey::generate();
        let rogue_pass = Pass {
            box_key: b.public(),
            link_key: thief,
            not_before: NOW,
            not_after: NOW + DAY,
        };
        assert!(rogue_pass.sign(&rogue).is_err());

        g.apply(b.public(), BoxState::Stolen);
        assert_eq!(g.decide(&b.public(), None), Err(PassRefused::Stolen));
        assert_eq!(g.decide(&phone, Some(&t)), Err(PassRefused::Stolen));

        g.apply(b.public(), BoxState::Gone);
        assert_eq!(g.decide(&phone, Some(&t)), Err(PassRefused::UnknownBox));
    }

    #[test]
    fn removed_devices() {
        let g = gate();
        let b = SecretKey::generate();
        g.apply(b.public(), BoxState::ActiveFactory);
        let phone = SecretKey::generate().public();
        let other = SecretKey::generate().public();
        let t = token(&b, &phone);
        let t2 = token(&b, &other);

        let list = |issued, full, devices: Vec<PublicKey>| RemovedDevices {
            box_key: b.public().into(),
            issued,
            full,
            devices: devices
                .into_iter()
                .map(|d| RemovedDevice {
                    link: d.into(),
                    until: NOW + 29 * DAY,
                })
                .collect(),
        };

        assert_eq!(g.take_removed(&list(NOW, false, vec![phone])), Ok(0));
        assert_eq!(g.decide(&phone, Some(&t)), Err(PassRefused::Removed));
        assert_eq!(g.decide(&other, Some(&t2)), Ok(b.public()));

        // A replay, or an older list, is refused.
        assert_eq!(
            g.take_removed(&list(NOW, true, vec![])),
            Err(ListRefused::Stale)
        );
        // Far from now.
        assert_eq!(
            g.take_removed(&list(NOW + 3600, true, vec![])),
            Err(ListRefused::Clock)
        );
        // Adding keeps the first; a full list replaces.
        assert_eq!(g.take_removed(&list(NOW + 1, false, vec![other])), Ok(0));
        assert_eq!(g.decide(&phone, Some(&t)), Err(PassRefused::Removed));
        assert_eq!(g.take_removed(&list(NOW + 2, true, vec![other])), Ok(0));
        assert_eq!(g.decide(&phone, Some(&t)), Ok(b.public()));
        assert_eq!(g.decide(&other, Some(&t2)), Err(PassRefused::Removed));

        // Unknown boxes can't send lists.
        let stranger = SecretKey::generate();
        let foreign = RemovedDevices {
            box_key: stranger.public().into(),
            issued: NOW,
            full: true,
            devices: vec![],
        };
        assert_eq!(g.take_removed(&foreign), Err(ListRefused::UnknownBox));
    }

    #[test]
    fn replacing_the_list_of_boxes() {
        let g = gate();
        let a = SecretKey::generate().public();
        let b = SecretKey::generate().public();
        g.replace_boxes([(a, BoxState::Active), (b, BoxState::Gone)]);
        assert_eq!(g.decide(&a, None), Ok(a));
        assert_eq!(g.decide(&b, None), Err(PassRefused::Missing));
        g.replace_boxes([(b, BoxState::Active)]);
        assert_eq!(g.decide(&a, None), Err(PassRefused::Missing));
        assert_eq!(g.boxes(), vec![(b, BoxState::Active)]);
    }
}
