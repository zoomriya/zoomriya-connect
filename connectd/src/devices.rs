//! The box's own list of paired devices: the only thing that decides who gets
//! in, on every path. No server, relay or pass is involved, so pairing and
//! connecting need neither ZOOMRIYA's servers nor the internet
//! (docs/direct-plus.md, §6).

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
use zoomriya_keys::{DeviceKey, Key, PublicKey};
use zoomriya_proto::link::{ConnectionReport, PairRequest};

/// How long a pairing code works.
pub const PAIRING_LIFETIME: u64 = 600;
/// Connection reports kept per device.
const REPORTS_KEPT: usize = 50;

/// One paired device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// A short ID for Box Desktop.
    pub id: String,
    /// What the family calls it, such as "Mum's phone".
    pub name: String,
    /// The device key in its security chip, pinned at pairing.
    pub device_key: String,
    /// Its link key: its iroh endpoint key.
    pub link: Key,
    /// An admin's device.
    #[serde(default)]
    pub admin: bool,
    /// When it paired.
    pub paired_at: u64,
    /// When it last connected.
    #[serde(default)]
    pub last_seen: u64,
    /// What iroh measured, newest last.
    #[serde(default)]
    pub reports: VecDeque<ConnectionReport>,
    /// A laptop, paired with a short code.
    #[serde(default)]
    pub laptop: bool,
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
    /// The pairing connection isn't from the link key the request names, or
    /// that key is already paired.
    #[error("wrong link key")]
    WrongLink,
}

impl Refused {
    /// A stable code.
    pub fn code(self) -> &'static str {
        match self {
            Refused::PairingCode => "pairing_code",
            Refused::DeviceSignature => "device_signature",
            Refused::WrongLink => "wrong_link",
        }
    }

    /// Plain words.
    pub fn message(self) -> &'static str {
        match self {
            Refused::PairingCode => {
                "That pairing code doesn't work. Show a new one on the box and try again."
            }
            Refused::DeviceSignature | Refused::WrongLink => {
                "Pairing didn't work. Update the ZOOMRIYA app and try again."
            }
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

/// A laptop's pairing code: short, so it can be typed, so it allows only a
/// few tries. Kept as it is, in memory only: SPAKE2 needs the code itself.
#[derive(Debug, Clone)]
struct LaptopPairing {
    code: String,
    expires: u64,
    name: String,
    tries_left: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    devices: Vec<Device>,
}

#[derive(Serialize)]
struct SavedRef<'a> {
    devices: &'a [Device],
}

/// The list, kept in a file in `/data`.
#[derive(Debug)]
pub struct Devices {
    file: PathBuf,
    list: Mutex<Vec<Device>>,
    pairing: Mutex<Option<Pairing>>,
    laptop_pairing: Mutex<Option<LaptopPairing>>,
}

fn sha(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

fn new_id() -> Result<String, Refused> {
    let mut id = [0u8; 6];
    getrandom::fill(&mut id).map_err(|_| Refused::PairingCode)?;
    Ok(HEXLOWER.encode(&id))
}

fn short(name: &str) -> String {
    name.trim().chars().take(60).collect()
}

impl Devices {
    /// Reads the list from `file`, or starts an empty one. Devices an older
    /// connectd marked removed are left out.
    pub fn open(file: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        struct Old {
            #[serde(default)]
            removed_at: Option<u64>,
        }
        let saved: Saved = match std::fs::read(file) {
            Ok(b) => {
                let raw: serde_json::Value = serde_json::from_slice(&b)
                    .with_context(|| format!("reading {}", file.display()))?;
                let mut saved: Saved = serde_json::from_value(raw.clone())
                    .with_context(|| format!("reading {}", file.display()))?;
                let old: Vec<Old> = raw
                    .get("devices")
                    .and_then(|d| serde_json::from_value(d.clone()).ok())
                    .unwrap_or_default();
                let mut keep = old.iter().map(|o| o.removed_at.is_none());
                saved.devices.retain(|_| keep.next().unwrap_or(true));
                saved
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            file: file.to_path_buf(),
            list: Mutex::new(saved.devices),
            pairing: Mutex::new(None),
            laptop_pairing: Mutex::new(None),
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

    /// Every device, newest first.
    pub fn all(&self) -> Vec<Device> {
        let mut v = self.list.lock().map(|l| l.clone()).unwrap_or_default();
        v.sort_by_key(|d| std::cmp::Reverse(d.paired_at));
        v
    }

    /// The paired device using this link key, if it's on the list.
    pub fn by_link(&self, link: &PublicKey) -> Option<Device> {
        self.list
            .lock()
            .ok()
            .and_then(|l| l.iter().find(|d| &d.link.0 == link).cloned())
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
            name: short(name),
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
    pub fn pair(&self, req: &PairRequest, from: &PublicKey, now: u64) -> Result<Device, Refused> {
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
        if &req.link.0 != from || self.by_link(from).is_some() {
            return Err(Refused::WrongLink);
        }
        let device_key =
            DeviceKey::from_b64(&req.device_key).map_err(|_| Refused::DeviceSignature)?;
        device_key
            .verify_link(&req.link.0, &req.link_sig)
            .map_err(|_| Refused::DeviceSignature)?;
        let device = Device {
            id: new_id()?,
            name: if pairing.name.is_empty() {
                short(&req.name)
            } else {
                pairing.name
            },
            device_key: device_key.to_b64(),
            link: req.link,
            admin: pairing.admin,
            paired_at: now,
            last_seen: now,
            reports: VecDeque::new(),
            laptop: false,
        };
        self.with(|l| l.push(device.clone()))
            .map_err(|_| Refused::PairingCode)?;
        Ok(device)
    }

    /// Opens pairing for a laptop: returns the short code Box Desktop shows.
    /// A new code replaces an older one.
    pub fn start_laptop_pairing(&self, name: &str, now: u64) -> Result<String> {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("randomness: {e}"))?;
        let code = zoomriya_proto::laptop::new_code(bytes);
        let mut p = self
            .laptop_pairing
            .lock()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        *p = Some(LaptopPairing {
            code: zoomriya_proto::laptop::normalise_code(&code),
            expires: now + PAIRING_LIFETIME,
            name: short(name),
            tries_left: zoomriya_proto::laptop::CODE_TRIES,
        });
        Ok(code)
    }

    /// Whether a laptop's pairing code is waiting.
    pub fn laptop_pairing_open(&self, now: u64) -> bool {
        self.laptop_pairing
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.expires > now && p.tries_left > 0))
            .unwrap_or(false)
    }

    /// Stops a laptop's pairing.
    pub fn cancel_laptop_pairing(&self) {
        if let Ok(mut p) = self.laptop_pairing.lock() {
            *p = None;
        }
    }

    /// A laptop starts pairing: uses up one try, and returns the code for
    /// the exchange ([`zoomriya_keys::pake`]) with the name Box Desktop gave.
    pub fn laptop_attempt(&self, now: u64) -> Result<(String, String), Refused> {
        let mut p = self
            .laptop_pairing
            .lock()
            .map_err(|_| Refused::PairingCode)?;
        let Some(open) = p.as_mut() else {
            return Err(Refused::PairingCode);
        };
        if now > open.expires || open.tries_left == 0 {
            *p = None;
            return Err(Refused::PairingCode);
        }
        open.tries_left -= 1;
        Ok((open.code.clone(), open.name.clone()))
    }

    /// Adds a laptop whose confirmation checked out, and closes its pairing.
    /// `name` is what Box Desktop called it, if anything; else the laptop's.
    pub fn finish_laptop(
        &self,
        req: &zoomriya_proto::laptop::LaptopFinish,
        link: &PublicKey,
        name: &str,
        now: u64,
    ) -> Result<Device, Refused> {
        let device_key =
            DeviceKey::from_b64(&req.device_key).map_err(|_| Refused::DeviceSignature)?;
        device_key
            .verify_link(link, &req.link_sig)
            .map_err(|_| Refused::DeviceSignature)?;
        if self.by_link(link).is_some() {
            return Err(Refused::WrongLink);
        }
        self.cancel_laptop_pairing();
        let device = Device {
            id: new_id()?,
            name: if name.is_empty() {
                short(&req.name)
            } else {
                short(name)
            },
            device_key: device_key.to_b64(),
            link: Key(*link),
            admin: false,
            paired_at: now,
            last_seen: now,
            reports: VecDeque::new(),
            laptop: true,
        };
        self.with(|l| l.push(device.clone()))
            .map_err(|_| Refused::PairingCode)?;
        Ok(device)
    }

    /// Removes a device from the list. Returns it, or `None` if there's no
    /// such device. Its connections are cut by the caller.
    pub fn remove(&self, id: &str) -> Result<Option<Device>> {
        self.with(|l| {
            let i = l.iter().position(|d| d.id == id)?;
            Some(l.remove(i))
        })
    }

    /// Renames a device.
    pub fn rename(&self, id: &str, name: &str) -> Result<bool> {
        self.with(|l| match l.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.name = short(name);
                true
            }
            None => false,
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use data_encoding::BASE64URL_NOPAD;
    use p256::ecdsa::{Signature, SigningKey, signature::Signer};
    use zoomriya_keys::{SecretKey, link_key_message};

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
        fn request(&self, secret: &str) -> PairRequest {
            PairRequest {
                secret: secret.into(),
                device_key: BASE64URL_NOPAD
                    .encode(self.chip.verifying_key().to_sec1_point(true).as_bytes()),
                link: self.link.public().into(),
                link_sig: self.sig(&self.link.public()),
                name: "Phone".into(),
            }
        }
        fn pair(&self, d: &Devices, secret: &str, now: u64) -> Result<Device, Refused> {
            d.pair(&self.request(secret), &self.link.public(), now)
        }
    }

    fn devices() -> (tempfile::TempDir, Devices) {
        let dir = tempfile::tempdir().expect("dir");
        let d = Devices::open(&dir.path().join("devices.json")).expect("open");
        (dir, d)
    }

    #[test]
    fn pairing_once() {
        let (dir, d) = devices();
        let phone = Phone::new();
        let secret = d.start_pairing("Mum's phone", true, NOW).expect("code");
        assert!(d.pairing_open(NOW));
        let device = phone.pair(&d, &secret, NOW).expect("pairs");
        assert_eq!(device.name, "Mum's phone");
        assert!(device.admin);
        assert!(d.by_link(&phone.link.public()).is_some());
        // The code is used up.
        assert!(!d.pairing_open(NOW));
        assert_eq!(
            Phone::new().pair(&d, &secret, NOW).map(|_| ()),
            Err(Refused::PairingCode)
        );
        // Kept on disk.
        let reopened = Devices::open(&dir.path().join("devices.json")).expect("open");
        assert!(reopened.by_link(&phone.link.public()).is_some());
    }

    #[test]
    fn pairing_needs_no_clock() {
        // A box that never reached the internet: its clock may say 1970.
        let (_dir, d) = devices();
        let phone = Phone::new();
        let secret = d.start_pairing("", false, 1_000).expect("code");
        assert!(phone.pair(&d, &secret, 1_000).is_ok());
    }

    #[test]
    fn pairing_refusals() {
        let (_dir, d) = devices();
        let phone = Phone::new();
        // Wrong secret uses the code up.
        let secret = d.start_pairing("", false, NOW).expect("code");
        assert_eq!(
            phone.pair(&d, "WRONG", NOW).map(|_| ()),
            Err(Refused::PairingCode)
        );
        assert_eq!(
            phone.pair(&d, &secret, NOW).map(|_| ()),
            Err(Refused::PairingCode)
        );
        // Too late.
        let secret = d.start_pairing("", false, NOW).expect("code");
        assert_eq!(
            phone
                .pair(&d, &secret, NOW + PAIRING_LIFETIME + 1)
                .map(|_| ()),
            Err(Refused::PairingCode)
        );
        // From another link key than the request names.
        let secret = d.start_pairing("", false, NOW).expect("code");
        assert_eq!(
            d.pair(
                &phone.request(&secret),
                &SecretKey::generate().public(),
                NOW
            )
            .map(|_| ()),
            Err(Refused::WrongLink)
        );
        // A link key the device key didn't sign.
        let secret = d.start_pairing("", false, NOW).expect("code");
        let mut req = phone.request(&secret);
        req.link_sig = Phone::new().sig(&phone.link.public());
        assert_eq!(
            d.pair(&req, &phone.link.public(), NOW).map(|_| ()),
            Err(Refused::DeviceSignature)
        );
        assert!(d.all().is_empty());
    }

    #[test]
    fn renaming_and_removing() {
        let (dir, d) = devices();
        let phone = Phone::new();
        let secret = d.start_pairing("", false, NOW).expect("code");
        let id = phone.pair(&d, &secret, NOW).expect("pairs").id;
        assert!(d.rename(&id, "  Dad's phone ").expect("io"));
        assert_eq!(d.all()[0].name, "Dad's phone");

        let removed = d.remove(&id).expect("io").expect("removed");
        assert_eq!(removed.link.0, phone.link.public());
        assert!(d.by_link(&phone.link.public()).is_none());
        assert!(d.all().is_empty());
        assert_eq!(d.remove(&id).expect("io"), None);
        assert!(!d.rename(&id, "x").expect("io"));
        let reopened = Devices::open(&dir.path().join("devices.json")).expect("open");
        assert!(reopened.all().is_empty());
    }

    #[test]
    fn older_lists_still_read() {
        // A list written by connectd with ZOOMRIYA Connect: passes, admin
        // keys, and a device removed but kept for the relays.
        let dir = tempfile::tempdir().expect("dir");
        let f = dir.path().join("devices.json");
        let a = SecretKey::generate().public().to_z32();
        let b = SecretKey::generate().public().to_z32();
        std::fs::write(
            &f,
            format!(
                r#"{{"devices":[
                {{"id":"a","name":"Kept","device_key":"x","link":"{a}","pass_until":5,"admin":true,"paired_at":1}},
                {{"id":"b","name":"Gone","device_key":"y","link":"{b}","pass_until":5,"admin":false,"paired_at":2,"removed_at":3}}
                ]}}"#
            ),
        )
        .expect("write");
        let d = Devices::open(&f).expect("reads");
        let all = d.all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Kept");
    }

    #[test]
    fn laptops_get_few_tries() {
        let (_dir, d) = devices();
        // No code shown: refused.
        assert_eq!(d.laptop_attempt(NOW).map(|_| ()), Err(Refused::PairingCode));

        // Three tries, then the code is used up.
        let code = d.start_laptop_pairing("Anna's laptop", NOW).expect("code");
        assert!(d.laptop_pairing_open(NOW));
        for _ in 0..3 {
            let (c, name) = d.laptop_attempt(NOW).expect("a try");
            assert_eq!(c, code.replace('-', ""));
            assert_eq!(name, "Anna's laptop");
        }
        assert!(!d.laptop_pairing_open(NOW));
        assert_eq!(d.laptop_attempt(NOW).map(|_| ()), Err(Refused::PairingCode));

        // Too late.
        d.start_laptop_pairing("", NOW).expect("code");
        assert_eq!(
            d.laptop_attempt(NOW + PAIRING_LIFETIME + 1).map(|_| ()),
            Err(Refused::PairingCode)
        );
    }

    #[test]
    fn laptops_are_added_once() {
        let (_dir, d) = devices();
        let laptop = Phone::new();
        let finish = |sig: String| zoomriya_proto::laptop::LaptopFinish {
            device_key: BASE64URL_NOPAD
                .encode(laptop.chip.verifying_key().to_sec1_point(false).as_bytes()),
            link_sig: sig,
            name: "MacBook".into(),
            confirm: String::new(),
        };
        let link = laptop.link.public();
        d.start_laptop_pairing("", NOW).expect("code");
        // Signed by another chip: refused.
        assert_eq!(
            d.finish_laptop(&finish(Phone::new().sig(&link)), &link, "", NOW)
                .map(|_| ()),
            Err(Refused::DeviceSignature)
        );
        let dev = d
            .finish_laptop(&finish(laptop.sig(&link)), &link, "", NOW)
            .expect("added");
        assert!(dev.laptop);
        assert_eq!(dev.name, "MacBook");
        assert!(!d.laptop_pairing_open(NOW), "one laptop per code");
        assert_eq!(
            d.finish_laptop(&finish(laptop.sig(&link)), &link, "", NOW)
                .map(|_| ()),
            Err(Refused::WrongLink)
        );
    }
}
