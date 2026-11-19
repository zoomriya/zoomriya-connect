//! The box's own list of paired devices: the only thing that decides who gets
//! in. Passes, relays and servers never do (docs/architecture.md, "connectd").

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result};
use data_encoding::{BASE32_NOPAD, HEXLOWER};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zoomriya_keys::{DeviceKey, Key, Pass, PublicKey, Purpose, SecretKey, seal};
use zoomriya_proto::{
    AdminNote, RemovedDevice,
    link::{ConnectionReport, PairRequest},
};

/// How long a pass lasts.
pub const PASS_LIFETIME: u64 = 30 * 86_400;
/// A pass is renewed when it has less than this left.
pub const RENEW_BEFORE: u64 = 21 * 86_400;
/// How long a pairing code works.
pub const PAIRING_LIFETIME: u64 = 600;
/// Connection reports kept per device.
const REPORTS_KEPT: usize = 50;
/// Passes are never signed with a clock earlier than this (November 2026):
/// a box whose clock was never set would sign passes that already ran out.
pub const EARLIEST_CLOCK: u64 = 1_793_000_000;

/// One paired device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// A short ID for Box Desktop.
    pub id: String,
    /// What the family calls it, such as "Mum's phone".
    pub name: String,
    /// The device key in its security chip, pinned at pairing.
    pub device_key: String,
    /// Its current link key.
    pub link: Key,
    /// When its current pass runs out.
    pub pass_until: u64,
    /// An admin's phone.
    pub admin: bool,
    /// The key its admin notes name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_key: Option<Key>,
    /// When it paired.
    pub paired_at: u64,
    /// When it last connected.
    #[serde(default)]
    pub last_seen: u64,
    /// When an admin removed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<u64>,
    /// What iroh measured, newest last.
    #[serde(default)]
    pub reports: VecDeque<ConnectionReport>,
}

/// Why something about devices was refused. `code()` and `message()` go to
/// the device or Box Desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// The pairing code is wrong, used or old.
    #[error("bad pairing code")]
    PairingCode,
    /// The device key's signature doesn't check out.
    #[error("bad device signature")]
    DeviceSignature,
    /// The pairing connection isn't from the link key the request names.
    #[error("wrong link key")]
    WrongLink,
    /// An admin's phone must send an admin key.
    #[error("no admin key")]
    NoAdminKey,
    /// The box's clock isn't set from the internet yet.
    #[error("clock not set")]
    Clock,
    /// Not on the list.
    #[error("unknown device")]
    Unknown,
}

impl Refused {
    /// A stable code.
    pub fn code(self) -> &'static str {
        match self {
            Refused::PairingCode => "pairing_code",
            Refused::DeviceSignature => "device_signature",
            Refused::WrongLink => "wrong_link",
            Refused::NoAdminKey => "no_admin_key",
            Refused::Clock => "clock",
            Refused::Unknown => "unknown_device",
        }
    }

    /// Plain words.
    pub fn message(self) -> &'static str {
        match self {
            Refused::PairingCode => {
                "That pairing code doesn't work. Show a new one on the box and scan it again."
            }
            Refused::DeviceSignature | Refused::WrongLink | Refused::NoAdminKey => {
                "Pairing didn't work. Update the ZOOMRIYA app and try again."
            }
            Refused::Clock => {
                "Your box can't get the time from the internet yet. Check its internet connection, then try again."
            }
            Refused::Unknown => "This device isn't paired with the box. Pair it again at home.",
        }
    }
}

#[derive(Debug, Clone)]
struct Pairing {
    secret_sha256: [u8; 32],
    expires: u64,
    name: String,
    admin: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    devices: Vec<Device>,
}

/// What a pairing gave the new device.
#[derive(Debug, Clone)]
pub struct Paired {
    /// The device as listed.
    pub device: Device,
    /// Its first pass.
    pub pass: String,
    /// For an admin's phone, its admin note.
    pub admin_note: Option<String>,
}

/// The list, kept in a file in `/data`.
#[derive(Debug)]
pub struct Devices {
    file: PathBuf,
    list: Mutex<Vec<Device>>,
    pairing: Mutex<Option<Pairing>>,
}

fn sha(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

impl Devices {
    /// Reads the list from `file`, or starts an empty one.
    pub fn open(file: &Path) -> Result<Self> {
        let saved: Saved = match std::fs::read(file) {
            Ok(b) => {
                serde_json::from_slice(&b).with_context(|| format!("reading {}", file.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            file: file.to_path_buf(),
            list: Mutex::new(saved.devices),
            pairing: Mutex::new(None),
        })
    }

    fn save(&self, list: &[Device]) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&SavedRef { devices: list })?;
        crate::files::write_private(&self.file, &bytes)
    }

    fn with<T>(&self, f: impl FnOnce(&mut Vec<Device>) -> T) -> Result<T> {
        let mut list = self.list.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let out = f(&mut list);
        self.save(&list)?;
        Ok(out)
    }

    /// Every device, removed ones included, newest first.
    pub fn all(&self) -> Vec<Device> {
        let mut v = self.list.lock().map(|l| l.clone()).unwrap_or_default();
        v.sort_by_key(|d| std::cmp::Reverse(d.paired_at));
        v
    }

    /// The paired device using this link key, if it's still on the list.
    pub fn by_link(&self, link: &PublicKey) -> Option<Device> {
        self.list.lock().ok().and_then(|l| {
            l.iter()
                .find(|d| d.removed_at.is_none() && &d.link.0 == link)
                .cloned()
        })
    }

    /// Opens pairing for one device: returns the secret for the QR code. A
    /// new code replaces an older one.
    pub fn start_pairing(&self, name: &str, admin: bool, now: u64) -> Result<String> {
        let mut bytes = [0u8; 20];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
        let secret = BASE32_NOPAD.encode(&bytes);
        let mut p = self.pairing.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        *p = Some(Pairing {
            secret_sha256: sha(&secret),
            expires: now + PAIRING_LIFETIME,
            name: name.trim().chars().take(60).collect(),
            admin,
        });
        Ok(secret)
    }

    /// Stops pairing.
    pub fn cancel_pairing(&self) {
        if let Ok(mut p) = self.pairing.lock() {
            *p = None;
        }
    }

    /// Whether a pairing code is waiting.
    pub fn pairing_open(&self, now: u64) -> bool {
        self.pairing
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.expires > now))
            .unwrap_or(false)
    }

    /// Pairs a device. `from` is the link key the pairing connection came
    /// from. Any failure uses the code up, so it can't be guessed at.
    pub fn pair(
        &self,
        req: &PairRequest,
        from: &PublicKey,
        box_key: &SecretKey,
        now: u64,
    ) -> Result<Paired, Refused> {
        let pairing = {
            let mut p = self.pairing.lock().map_err(|_| Refused::PairingCode)?;
            let Some(open) = p.take() else {
                return Err(Refused::PairingCode);
            };
            let right: bool = sha(&req.secret).ct_eq(&open.secret_sha256).into();
            if !right || now > open.expires {
                return Err(Refused::PairingCode);
            }
            open
        };
        if &req.link.0 != from {
            return Err(Refused::WrongLink);
        }
        let device_key =
            DeviceKey::from_b64(&req.device_key).map_err(|_| Refused::DeviceSignature)?;
        device_key
            .verify_link(&req.link.0, &req.link_sig)
            .map_err(|_| Refused::DeviceSignature)?;
        if pairing.admin && req.admin_key.is_none() {
            return Err(Refused::NoAdminKey);
        }
        let (pass, until) = issue_pass(box_key, &req.link.0, now)?;
        let mut id = [0u8; 6];
        getrandom::fill(&mut id).map_err(|_| Refused::PairingCode)?;
        let name = if pairing.name.is_empty() {
            req.name.trim().chars().take(60).collect()
        } else {
            pairing.name.clone()
        };
        let device = Device {
            id: HEXLOWER.encode(&id),
            name,
            device_key: device_key.to_b64(),
            link: req.link,
            pass_until: until,
            admin: pairing.admin,
            admin_key: if pairing.admin { req.admin_key } else { None },
            paired_at: now,
            last_seen: now,
            removed_at: None,
            reports: VecDeque::new(),
        };
        let admin_note = device.admin_key.map(|k| {
            seal(
                box_key,
                Purpose::AdminNote,
                &AdminNote {
                    box_key: box_key.public().into(),
                    admin: k,
                    issued: now,
                },
            )
        });
        self.with(|l| l.push(device.clone()))
            .map_err(|_| Refused::PairingCode)?;
        Ok(Paired {
            device,
            pass,
            admin_note,
        })
    }

    /// A new link key for a device, vouched for by its device key: returns
    /// the new pass. The old link key stops working at once.
    pub fn renew(
        &self,
        id: &str,
        new_link: &PublicKey,
        sig: &str,
        box_key: &SecretKey,
        now: u64,
    ) -> Result<(String, u64), Refused> {
        let device = self
            .all()
            .into_iter()
            .find(|d| d.id == id && d.removed_at.is_none())
            .ok_or(Refused::Unknown)?;
        DeviceKey::from_b64(&device.device_key)
            .and_then(|k| k.verify_link(new_link, sig))
            .map_err(|_| Refused::DeviceSignature)?;
        let (pass, until) = issue_pass(box_key, new_link, now)?;
        self.with(|l| {
            if let Some(d) = l.iter_mut().find(|d| d.id == id) {
                d.link = Key(*new_link);
                d.pass_until = until;
            }
        })
        .map_err(|_| Refused::Unknown)?;
        Ok((pass, until))
    }

    /// Whether a device's pass should be renewed now.
    pub fn needs_renewal(device: &Device, now: u64) -> bool {
        device.pass_until.saturating_sub(now) < RENEW_BEFORE
    }

    /// Removes a device. Returns what the relay must refuse, or `None` if
    /// there's no such device.
    pub fn remove(&self, id: &str, now: u64) -> Result<Option<RemovedDevice>> {
        self.with(|l| {
            let d = l
                .iter_mut()
                .find(|d| d.id == id && d.removed_at.is_none())?;
            d.removed_at = Some(now);
            Some(RemovedDevice {
                link: d.link,
                until: d.pass_until,
            })
        })
    }

    /// Renames a device.
    pub fn rename(&self, id: &str, name: &str) -> Result<bool> {
        self.with(|l| match l.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.name = name.trim().chars().take(60).collect();
                true
            }
            None => false,
        })
    }

    /// Every removed device whose last pass hasn't run out: the relay's list.
    /// Removed devices whose passes ran out over a month ago are forgotten.
    pub fn removed_list(&self, now: u64) -> Vec<RemovedDevice> {
        let _ = self
            .with(|l| l.retain(|d| d.removed_at.is_none() || d.pass_until + PASS_LIFETIME > now));
        self.all()
            .into_iter()
            .filter(|d| d.removed_at.is_some() && d.pass_until > now)
            .map(|d| RemovedDevice {
                link: d.link,
                until: d.pass_until,
            })
            .collect()
    }

    /// Notes that a device connected.
    pub fn seen(&self, id: &str, now: u64) {
        let _ = self.with(|l| {
            if let Some(d) = l.iter_mut().find(|d| d.id == id) {
                d.last_seen = now;
            }
        });
    }

    /// Keeps a device's connection report.
    pub fn report(&self, id: &str, report: ConnectionReport) {
        let _ = self.with(|l| {
            if let Some(d) = l.iter_mut().find(|d| d.id == id) {
                d.reports.push_back(report);
                while d.reports.len() > REPORTS_KEPT {
                    d.reports.pop_front();
                }
            }
        });
    }
}

#[derive(Serialize)]
struct SavedRef<'a> {
    devices: &'a [Device],
}

/// Signs a 30-day pass, if the clock can be trusted.
fn issue_pass(box_key: &SecretKey, link: &PublicKey, now: u64) -> Result<(String, u64), Refused> {
    if now < EARLIEST_CLOCK || !crate::clock::synced() {
        return Err(Refused::Clock);
    }
    let until = now + PASS_LIFETIME;
    let pass = Pass {
        box_key: box_key.public(),
        link_key: *link,
        // A minute back, for a phone whose clock runs slightly behind.
        not_before: now.saturating_sub(60),
        not_after: until,
    }
    .sign(box_key)
    .map_err(|_| Refused::Clock)?;
    Ok((pass, until))
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_encoding::BASE64URL_NOPAD;
    use p256::ecdsa::{Signature, SigningKey, signature::Signer};
    use zoomriya_keys::link_key_message;

    const NOW: u64 = 1_800_000_000;

    struct Phone {
        chip: SigningKey,
        link: SecretKey,
    }

    impl Phone {
        fn new() -> Self {
            Self {
                chip: zoomriya_keys::new_test_chip(),
                link: SecretKey::generate(),
            }
        }
        fn sig(&self, link: &PublicKey) -> String {
            let s: Signature = self.chip.sign(&link_key_message(link));
            BASE64URL_NOPAD.encode(s.to_der().as_bytes())
        }
        fn request(&self, secret: &str, admin_key: Option<Key>) -> PairRequest {
            PairRequest {
                secret: secret.into(),
                device_key: BASE64URL_NOPAD
                    .encode(self.chip.verifying_key().to_sec1_point(true).as_bytes()),
                link: self.link.public().into(),
                link_sig: self.sig(&self.link.public()),
                name: "Phone".into(),
                admin_key,
            }
        }
    }

    fn devices() -> (tempfile::TempDir, Devices) {
        crate::clock::assume_synced_for_tests();
        let dir = tempfile::tempdir().expect("dir");
        let d = Devices::open(&dir.path().join("devices.json")).expect("open");
        (dir, d)
    }

    #[test]
    fn pairing_once() {
        let (dir, d) = devices();
        let b = SecretKey::generate();
        let phone = Phone::new();
        let secret = d.start_pairing("Mum's phone", false, NOW).expect("code");
        assert!(d.pairing_open(NOW));
        let paired = d
            .pair(&phone.request(&secret, None), &phone.link.public(), &b, NOW)
            .expect("pairs");
        assert_eq!(paired.device.name, "Mum's phone");
        assert!(paired.admin_note.is_none());
        let pass = Pass::open(&paired.pass).expect("pass");
        assert_eq!(pass.link_key, phone.link.public());
        assert_eq!(pass.box_key, b.public());
        assert!(d.by_link(&phone.link.public()).is_some());
        // The code is used up.
        assert!(!d.pairing_open(NOW));
        let again = Phone::new();
        assert_eq!(
            d.pair(&again.request(&secret, None), &again.link.public(), &b, NOW)
                .map(|_| ()),
            Err(Refused::PairingCode)
        );
        // Kept on disk.
        let reopened = Devices::open(&dir.path().join("devices.json")).expect("open");
        assert!(reopened.by_link(&phone.link.public()).is_some());
    }

    #[test]
    fn pairing_refusals() {
        let (_dir, d) = devices();
        let b = SecretKey::generate();
        let phone = Phone::new();
        // Wrong secret uses the code up.
        let secret = d.start_pairing("", false, NOW).expect("code");
        assert_eq!(
            d.pair(&phone.request("WRONG", None), &phone.link.public(), &b, NOW)
                .map(|_| ()),
            Err(Refused::PairingCode)
        );
        assert_eq!(
            d.pair(&phone.request(&secret, None), &phone.link.public(), &b, NOW)
                .map(|_| ()),
            Err(Refused::PairingCode)
        );
        // Too late.
        let secret = d.start_pairing("", false, NOW).expect("code");
        assert_eq!(
            d.pair(
                &phone.request(&secret, None),
                &phone.link.public(),
                &b,
                NOW + PAIRING_LIFETIME + 1
            )
            .map(|_| ()),
            Err(Refused::PairingCode)
        );
        // From another link key than the request names.
        let secret = d.start_pairing("", false, NOW).expect("code");
        assert_eq!(
            d.pair(
                &phone.request(&secret, None),
                &SecretKey::generate().public(),
                &b,
                NOW
            )
            .map(|_| ()),
            Err(Refused::WrongLink)
        );
        // A link key the device key didn't sign.
        let secret = d.start_pairing("", false, NOW).expect("code");
        let mut req = phone.request(&secret, None);
        req.link_sig = Phone::new().sig(&phone.link.public());
        assert_eq!(
            d.pair(&req, &phone.link.public(), &b, NOW).map(|_| ()),
            Err(Refused::DeviceSignature)
        );
        // An admin's phone without an admin key.
        let secret = d.start_pairing("", true, NOW).expect("code");
        assert_eq!(
            d.pair(&phone.request(&secret, None), &phone.link.public(), &b, NOW)
                .map(|_| ()),
            Err(Refused::NoAdminKey)
        );
        assert!(d.all().is_empty());
    }

    #[test]
    fn admins_get_notes() {
        let (_dir, d) = devices();
        let b = SecretKey::generate();
        let phone = Phone::new();
        let admin_key = SecretKey::generate().public();
        let secret = d.start_pairing("Dad", true, NOW).expect("code");
        let paired = d
            .pair(
                &phone.request(&secret, Some(admin_key.into())),
                &phone.link.public(),
                &b,
                NOW,
            )
            .expect("pairs");
        let note: AdminNote = zoomriya_keys::open(
            Purpose::AdminNote,
            &paired.admin_note.expect("note"),
            1024,
            |n: &AdminNote| n.box_key.0,
        )
        .expect("box signed it");
        assert_eq!(note.admin.0, admin_key);
    }

    #[test]
    fn renewing_and_removing() {
        let (_dir, d) = devices();
        let b = SecretKey::generate();
        let phone = Phone::new();
        let secret = d.start_pairing("", false, NOW).expect("code");
        let id = d
            .pair(&phone.request(&secret, None), &phone.link.public(), &b, NOW)
            .expect("pairs")
            .device
            .id;
        let dev = d.by_link(&phone.link.public()).expect("there");
        assert!(!Devices::needs_renewal(&dev, NOW + 8 * 86_400));
        assert!(Devices::needs_renewal(&dev, NOW + 10 * 86_400));

        // A new link key, signed by the chip.
        let next = SecretKey::generate().public();
        let (pass, until) = d
            .renew(&id, &next, &phone.sig(&next), &b, NOW + 10 * 86_400)
            .expect("renews");
        assert_eq!(until, NOW + 10 * 86_400 + PASS_LIFETIME);
        assert_eq!(Pass::open(&pass).expect("pass").link_key, next);
        assert!(d.by_link(&phone.link.public()).is_none(), "old key stops");
        assert!(d.by_link(&next).is_some());
        // A link key the chip didn't sign.
        let stolen = SecretKey::generate().public();
        assert_eq!(
            d.renew(&id, &stolen, &Phone::new().sig(&stolen), &b, NOW)
                .map(|_| ()),
            Err(Refused::DeviceSignature)
        );

        // Removed: off the list, on the relay's.
        let r = d
            .remove(&id, NOW + 11 * 86_400)
            .expect("io")
            .expect("removed");
        assert_eq!(r.link.0, next);
        assert!(d.by_link(&next).is_none());
        assert_eq!(d.removed_list(NOW + 11 * 86_400).len(), 1);
        assert_eq!(d.remove(&id, NOW).expect("io"), None);
        // After its pass ran out, the relay needn't know.
        assert!(d.removed_list(until + 1).is_empty());
        // And a month later it's forgotten.
        d.removed_list(until + PASS_LIFETIME + 1);
        assert!(d.all().is_empty());
    }

    #[test]
    fn no_passes_without_the_time() {
        let (_dir, d) = devices();
        let b = SecretKey::generate();
        let phone = Phone::new();
        let secret = d.start_pairing("", false, 1_000).expect("code");
        assert_eq!(
            d.pair(
                &phone.request(&secret, None),
                &phone.link.public(),
                &b,
                1_000
            )
            .map(|_| ()),
            Err(Refused::Clock)
        );
    }
}
