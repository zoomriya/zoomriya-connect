//! Registrations, in SQLite, and an in-memory copy of what DNS answers, so DNS
//! never waits on the database.
//!
//! The operator commands run in another process on the same file; the server
//! notices their changes within a second (`PRAGMA data_version`) and reloads.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    sync::{
        Mutex, RwLock,
        atomic::{AtomicU32, Ordering},
    },
};

use anyhow::{Context, Result};
use data_encoding::HEXLOWER;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use zoomriya_keys::{Key, PublicKey};
use zoomriya_proto::{
    AcmeTarget, BoxChange, BoxFeed, BoxState, HomeAddrs, RegisterRequest,
    names::{self, NameRules},
};

/// How long the guests' door stays in DNS without the box renewing it.
pub const DOOR_LIFETIME: u64 = 24 * 3600;
/// How long certificate-check records stay without being renewed.
pub const ACME_LIFETIME: u64 = 3600;
/// Most changes in one feed answer.
const FEED_PAGE: i64 = 10_000;

/// Why the store refused something. `code` and `message` go to the box as
/// they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// HTTP status.
    pub status: u16,
    /// Stable code.
    pub code: &'static str,
    /// Plain words for Box Desktop.
    pub message: String,
}

impl Refused {
    fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    fn internal(err: impl std::fmt::Display) -> Self {
        tracing::error!("database: {err}");
        Self::new(
            500,
            "internal",
            "Something went wrong on our side. Try again in a minute.",
        )
    }
}

/// What DNS needs about one box.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// Where the name points.
    pub home: HomeAddrs,
    /// The guests' door, while it's open.
    pub door_v4: Option<Ipv4Addr>,
    /// The guests' door's own IPv6 address.
    pub door_v6: Option<Ipv6Addr>,
    /// When the door drops out of DNS unless renewed.
    pub door_until: u64,
    /// Certificate-check records for the box's name.
    pub acme_box: Vec<String>,
    /// Certificate-check records for the door's name.
    pub acme_door: Vec<String>,
    /// When the check records drop out.
    pub acme_until: u64,
}

/// One registration, as the API needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxRow {
    /// The name.
    pub name: String,
    /// The box key.
    pub box_key: PublicKey,
    /// The DNS update key.
    pub dns_update_key: PublicKey,
    /// Registered with a factory token.
    pub factory: bool,
    /// Marked stolen.
    pub stolen: bool,
    /// Switched off by us.
    pub disabled: bool,
}

/// One box, as the GUI lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BoxSummary {
    /// The name.
    pub name: String,
    /// The box key, z-base-32.
    pub key: String,
    /// Registered with a factory token.
    pub factory: bool,
    /// Marked stolen.
    pub stolen: bool,
    /// Switched off by us.
    pub disabled: bool,
    /// When it registered.
    pub registered_at: u64,
    /// When the guests' door drops out (0: closed).
    pub door_until: u64,
    /// Whether it has published an address record.
    pub has_record: bool,
    /// Bytes through our relays this month.
    pub used: u64,
    /// Its own allowance in GB, if an operator set one (0: none).
    pub own_allowance_gb: Option<u32>,
    /// The account it's linked to.
    pub account: Option<String>,
}

/// What a registration did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    /// The name.
    pub name: String,
    /// With a factory token.
    pub factory: bool,
    /// `false` when the box registered again (same name, same key).
    pub new: bool,
    /// The account it's linked to.
    pub account: Option<String>,
}

/// The rules a registration is checked against.
#[derive(Debug, Clone)]
pub struct Rules {
    /// Name rules, with the operators' extra words.
    pub names: NameRules,
    /// New boxes a day per address.
    pub per_address_per_day: u32,
    /// Days a released name waits.
    pub released_name_days: u32,
    /// Every box needs an account, issued by an operator, to register.
    pub accounts_required: bool,
}

/// The monthly relay allowances, set in the GUI. 0 means no limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Allowances {
    /// GB a month for a self-installed box.
    pub default_gb: u32,
    /// GB a month for a ready-made box (with a factory token).
    pub factory_gb: u32,
    /// How fast a box past its allowance may go, in kilobytes a second.
    pub slow_kbps: u32,
}

impl Default for Allowances {
    /// No limit until an operator sets one in the GUI, so a family's own
    /// server never limits its own family.
    fn default() -> Self {
        Self {
            default_gb: 0,
            factory_gb: 0,
            slow_kbps: 125,
        }
    }
}

/// A GB, as phone plans count it.
pub const GB: u64 = 1_000_000_000;

/// An account, for checking its sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// Its number.
    pub id: i64,
    /// The username.
    pub username: String,
    /// The password's Argon2 hash.
    pub password: String,
    /// Blocked by an operator.
    pub blocked: bool,
    /// Locked after wrong passwords until then (seconds since 1970).
    pub locked_until: u64,
}

/// An account, as the GUI lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AccountSummary {
    /// The username.
    pub username: String,
    /// Blocked by an operator.
    pub blocked: bool,
    /// How many boxes it may link.
    pub max_boxes: u32,
    /// Its boxes' names.
    pub boxes: Vec<String>,
    /// When it was made.
    pub created_at: u64,
    /// Locked after wrong passwords until then (0: not locked).
    pub locked_until: u64,
}

/// Wrong passwords before an account is locked for a while.
pub const WRONG_PASSWORDS: u32 = 5;
/// How long it's locked.
pub const LOCKED_FOR: u64 = 15 * 60;

/// The store.
pub struct Store {
    db: Mutex<Connection>,
    zone: RwLock<HashMap<String, Entry>>,
    records: RwLock<HashMap<PublicKey, Vec<u8>>>,
    serial: AtomicU32,
    data_version: Mutex<i64>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Store")
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS boxes (
    name            TEXT PRIMARY KEY,
    box_key         TEXT NOT NULL UNIQUE,
    dns_update_key  TEXT NOT NULL,
    factory         INTEGER NOT NULL DEFAULT 0,
    stolen          INTEGER NOT NULL DEFAULT 0,
    disabled        INTEGER NOT NULL DEFAULT 0,
    home            TEXT NOT NULL DEFAULT '{}',
    door_v4         TEXT,
    door_v6         TEXT,
    door_until      INTEGER NOT NULL DEFAULT 0,
    acme_box        TEXT NOT NULL DEFAULT '[]',
    acme_door       TEXT NOT NULL DEFAULT '[]',
    acme_until      INTEGER NOT NULL DEFAULT 0,
    registered_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS released (
    name     TEXT PRIMARY KEY,
    box_key  TEXT NOT NULL,
    until    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS registration_log (
    at    INTEGER NOT NULL,
    addr  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS registration_log_addr ON registration_log (addr, at);
CREATE TABLE IF NOT EXISTS factory_tokens (
    sha256   TEXT PRIMARY KEY,
    used_by  TEXT,
    used_at  INTEGER
);
CREATE TABLE IF NOT EXISTS box_events (
    seq      INTEGER PRIMARY KEY AUTOINCREMENT,
    box_key  TEXT NOT NULL,
    state    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS address_records (
    box_key  TEXT PRIMARY KEY,
    payload  BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS relay_use (
    box_key  TEXT NOT NULL,
    month    TEXT NOT NULL,
    bytes    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (box_key, month)
);
CREATE TABLE IF NOT EXISTS allowances (
    box_key  TEXT PRIMARY KEY,
    gb       INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS accounts (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    username         TEXT NOT NULL UNIQUE,
    password         TEXT NOT NULL,
    blocked          INTEGER NOT NULL DEFAULT 0,
    max_boxes        INTEGER NOT NULL DEFAULT 3,
    failed           INTEGER NOT NULL DEFAULT 0,
    locked_until     INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    password_set_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS account_sessions (
    token_hash  TEXT PRIMARY KEY,
    account_id  INTEGER NOT NULL,
    expires     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    name   TEXT PRIMARY KEY,
    value  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reports (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    name     TEXT NOT NULL,
    reason   TEXT NOT NULL,
    at       INTEGER NOT NULL,
    addr     TEXT NOT NULL,
    handled  INTEGER NOT NULL DEFAULT 0
);
"#;

fn state_str(s: BoxState) -> &'static str {
    match s {
        BoxState::Active => "active",
        BoxState::ActiveFactory => "active_factory",
        BoxState::Stolen => "stolen",
        BoxState::Gone => "gone",
    }
}

fn state_from(s: &str) -> BoxState {
    match s {
        "active" => BoxState::Active,
        "active_factory" => BoxState::ActiveFactory,
        "stolen" => BoxState::Stolen,
        _ => BoxState::Gone,
    }
}

fn key_col(s: &str) -> rusqlite::Result<PublicKey> {
    PublicKey::from_z32(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Where registrations from one home are counted: the IPv4 address, or the
/// IPv6 /64.
pub fn address_bucket(addr: IpAddr) -> String {
    match addr {
        IpAddr::V4(a) => a.to_string(),
        IpAddr::V6(a) => match a.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = a.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

/// How factory tokens are kept: only their SHA-256.
pub fn token_hash(token: &str) -> String {
    HEXLOWER.encode(&Sha256::digest(token.trim().as_bytes()))
}

impl Store {
    /// Opens (or makes) the database and loads what DNS needs.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        }
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::with_connection(db)
    }

    /// A store in memory, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::open_memory()
    }

    /// A store kept only in memory: the second DNS server's.
    pub fn open_memory() -> Result<Self> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.execute_batch(SCHEMA)?;
        // Added after the first release: which account a box is linked to.
        let has_account: bool = db
            .prepare("SELECT 1 FROM pragma_table_info('boxes') WHERE name = 'account_id'")?
            .exists([])?;
        if !has_account {
            db.execute_batch("ALTER TABLE boxes ADD COLUMN account_id INTEGER")?;
        }
        let version: i64 = db.query_row("PRAGMA data_version", [], |r| r.get(0))?;
        let store = Self {
            db: Mutex::new(db),
            zone: RwLock::default(),
            records: RwLock::default(),
            serial: AtomicU32::new(zoomriya_serve::now() as u32),
            data_version: Mutex::new(version),
        };
        store.reload()?;
        Ok(store)
    }

    fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>, Refused> {
        self.db.lock().map_err(Refused::internal)
    }

    /// Reads everything DNS needs from the database again.
    pub fn reload(&self) -> Result<()> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut zone = HashMap::new();
        {
            let mut q = db.prepare(
                "SELECT name, home, door_v4, door_v6, door_until, acme_box, acme_door, acme_until
                 FROM boxes b WHERE disabled = 0 AND stolen = 0
                   AND (b.account_id IS NULL OR b.account_id NOT IN (SELECT id FROM accounts WHERE blocked = 1))",
            )?;
            let rows = q.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    entry_from_row(
                        &r.get::<_, String>(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        &r.get::<_, String>(5)?,
                        &r.get::<_, String>(6)?,
                        r.get(7)?,
                    ),
                ))
            })?;
            for row in rows {
                let (name, entry) = row?;
                zone.insert(name, entry);
            }
        }
        let mut records = HashMap::new();
        {
            let mut q = db.prepare(
                "SELECT a.box_key, a.payload FROM address_records a
                 JOIN boxes b ON b.box_key = a.box_key WHERE b.stolen = 0",
            )?;
            let rows = q.query_map([], |r| {
                Ok((key_col(&r.get::<_, String>(0)?)?, r.get::<_, Vec<u8>>(1)?))
            })?;
            for row in rows {
                let (k, p) = row?;
                records.insert(k, p);
            }
        }
        drop(db);
        if let Ok(mut z) = self.zone.write() {
            *z = zone;
        }
        if let Ok(mut r) = self.records.write() {
            *r = records;
        }
        self.bump();
        Ok(())
    }

    /// Reloads if another process changed the database. Cheap: call it every second.
    pub fn reload_if_changed(&self) -> Result<bool> {
        let version: i64 = {
            let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            db.query_row("PRAGMA data_version", [], |r| r.get(0))?
        };
        let changed = {
            let mut v = self
                .data_version
                .lock()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let changed = *v != version;
            *v = version;
            changed
        };
        if changed {
            self.reload()?;
        }
        Ok(changed)
    }

    fn bump(&self) {
        let now = zoomriya_serve::now() as u32;
        let _ = self
            .serial
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |s| {
                Some(now.max(s.wrapping_add(1)))
            });
    }

    /// The zone's serial number, which grows with each change.
    pub fn serial(&self) -> u32 {
        self.serial.load(Ordering::Relaxed)
    }

    /// What DNS has for a name.
    pub fn entry(&self, name: &str) -> Option<Entry> {
        self.zone.read().ok().and_then(|z| z.get(name).cloned())
    }

    /// A box's address record (signature, time and DNS packet, as iroh sends it).
    pub fn record(&self, key: &PublicKey) -> Option<Vec<u8>> {
        self.records.read().ok().and_then(|r| r.get(key).cloned())
    }

    fn put_entry(&self, name: &str, f: impl FnOnce(&mut Entry)) {
        if let Ok(mut z) = self.zone.write()
            && let Some(e) = z.get_mut(name)
        {
            f(e);
        }
        self.bump();
    }

    /// A registration by name.
    pub fn find(&self, name: &str) -> Result<Option<BoxRow>, Refused> {
        let db = self.db()?;
        find(&db, "name", name).map_err(Refused::internal)
    }

    /// A registration by box key.
    pub fn find_by_key(&self, key: &PublicKey) -> Result<Option<BoxRow>, Refused> {
        let db = self.db()?;
        find(&db, "box_key", &key.to_z32()).map_err(Refused::internal)
    }

    /// Registers a box, or registers it again (same name and key: takes the
    /// new DNS update key and addresses). The request's signature is already
    /// checked to be the box key's.
    /// Registers a box. `account` is the account whose sign-in came with the
    /// request, already checked.
    pub fn register(
        &self,
        req: &RegisterRequest,
        from: IpAddr,
        now: u64,
        rules: &Rules,
        account: Option<i64>,
    ) -> Result<Registered, Refused> {
        let name = names::normalise(&req.name);
        if let Err(why) = rules.names.check(&name) {
            return Err(Refused::new(400, why.code(), why.message()));
        }
        check_home(&req.home)?;
        let box_key = req.box_key.0.to_z32();
        let home = serde_json::to_string(&req.home).map_err(Refused::internal)?;
        let mut db = self.db()?;
        let tx = db.transaction().map_err(Refused::internal)?;

        let by_name = find(&tx, "name", &name).map_err(Refused::internal)?;
        let by_key = find(&tx, "box_key", &box_key).map_err(Refused::internal)?;
        // Registering again.
        if let Some(row) = &by_name
            && row.box_key == req.box_key.0
        {
            if row.stolen {
                return Err(Refused::new(403, "stolen", "This box was marked stolen."));
            }
            if row.disabled {
                return Err(name_taken());
            }
            let linked: Option<i64> = tx
                .query_row(
                    "SELECT account_id FROM boxes WHERE name = ?1",
                    params![name],
                    |r| r.get(0),
                )
                .map_err(Refused::internal)?;
            let account_id = match (linked, account) {
                (Some(a), _) => Some(a),
                (None, Some(a)) => {
                    account_has_room(&tx, a)?;
                    Some(a)
                }
                (None, None) if rules.accounts_required => return Err(account_needed()),
                (None, None) => None,
            };
            let username = match account_id {
                Some(a) => Some(account_usable(&tx, a)?),
                None => None,
            };
            tx.execute(
                "UPDATE boxes SET dns_update_key = ?1, home = ?2, account_id = ?3 WHERE name = ?4",
                params![req.dns_update_key.0.to_z32(), home, account_id, name],
            )
            .map_err(Refused::internal)?;
            tx.commit().map_err(Refused::internal)?;
            let factory = row.factory;
            self.put_entry(&name, |e| e.home = req.home.clone());
            return Ok(Registered {
                name,
                factory,
                new: false,
                account: username,
            });
        }
        if by_name.is_some() {
            return Err(name_taken());
        }
        if by_key.is_some() {
            return Err(Refused::new(
                409,
                "box_has_name",
                "This box already has a name. Give it up first to pick another.",
            ));
        }
        let waiting: Option<String> = tx
            .query_row(
                "SELECT box_key FROM released WHERE name = ?1 AND until > ?2",
                params![name, now],
                |r| r.get(0),
            )
            .optional()
            .map_err(Refused::internal)?;
        if waiting.is_some_and(|k| k != box_key) {
            return Err(name_taken());
        }
        if account.is_none() && rules.accounts_required {
            return Err(account_needed());
        }
        let username = match account {
            Some(a) => {
                account_has_room(&tx, a)?;
                Some(account_usable(&tx, a)?)
            }
            None => None,
        };

        // A factory token, or the daily limit per address.
        let factory = match &req.factory_token {
            Some(token) => {
                let n = tx
                    .execute(
                        "UPDATE factory_tokens SET used_by = ?1, used_at = ?2
                         WHERE sha256 = ?3 AND used_by IS NULL",
                        params![box_key, now, token_hash(token)],
                    )
                    .map_err(Refused::internal)?;
                if n == 0 {
                    return Err(Refused::new(
                        403,
                        "factory_token",
                        "This box's factory token isn't valid, or was already used. Contact ZOOMRIYA support.",
                    ));
                }
                true
            }
            None => {
                let bucket = address_bucket(from);
                let count: u32 = tx
                    .query_row(
                        "SELECT COUNT(*) FROM registration_log WHERE addr = ?1 AND at > ?2",
                        params![bucket, now.saturating_sub(86_400)],
                        |r| r.get(0),
                    )
                    .map_err(Refused::internal)?;
                if count >= rules.per_address_per_day {
                    return Err(Refused::new(
                        429,
                        "too_many_boxes",
                        "Too many new boxes from your internet connection today. Try again tomorrow.",
                    ));
                }
                tx.execute(
                    "INSERT INTO registration_log (at, addr) VALUES (?1, ?2)",
                    params![now, bucket],
                )
                .map_err(Refused::internal)?;
                false
            }
        };

        tx.execute(
            "INSERT INTO boxes (name, box_key, dns_update_key, factory, home, registered_at, account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                name,
                box_key,
                req.dns_update_key.0.to_z32(),
                factory,
                home,
                now,
                account
            ],
        )
        .map_err(Refused::internal)?;
        tx.execute("DELETE FROM released WHERE name = ?1", params![name])
            .map_err(Refused::internal)?;
        let state = if factory {
            BoxState::ActiveFactory
        } else {
            BoxState::Active
        };
        event(&tx, &box_key, state).map_err(Refused::internal)?;
        tx.execute(
            "DELETE FROM registration_log WHERE at <= ?1",
            params![now.saturating_sub(86_400)],
        )
        .map_err(Refused::internal)?;
        tx.commit().map_err(Refused::internal)?;

        if let Ok(mut z) = self.zone.write() {
            z.insert(
                name.clone(),
                Entry {
                    home: req.home.clone(),
                    ..Entry::default()
                },
            );
        }
        self.bump();
        Ok(Registered {
            name,
            factory,
            new: true,
            account: username,
        })
    }

    /// Where the name points.
    pub fn set_home(&self, name: &str, home: &HomeAddrs) -> Result<(), Refused> {
        check_home(home)?;
        let json = serde_json::to_string(home).map_err(Refused::internal)?;
        self.update("home = ?1", params![json, name])?;
        self.put_entry(name, |e| e.home = home.clone());
        Ok(())
    }

    /// The certificate-check records. Empty removes them.
    pub fn set_acme(
        &self,
        name: &str,
        target: AcmeTarget,
        values: &[String],
        now: u64,
    ) -> Result<(), Refused> {
        if values.len() > 4
            || values
                .iter()
                .any(|v| v.is_empty() || v.len() > 255 || !v.is_ascii())
        {
            return Err(Refused::new(
                400,
                "bad_acme",
                "Up to four check values, each up to 255 plain characters.",
            ));
        }
        let json = serde_json::to_string(values).map_err(Refused::internal)?;
        let until = now + ACME_LIFETIME;
        let col = match target {
            AcmeTarget::Box => "acme_box",
            AcmeTarget::Door => "acme_door",
        };
        self.update(
            &format!("{col} = ?1, acme_until = ?2"),
            params![json, until, name],
        )?;
        self.put_entry(name, |e| {
            match target {
                AcmeTarget::Box => e.acme_box = values.to_vec(),
                AcmeTarget::Door => e.acme_door = values.to_vec(),
            }
            e.acme_until = until;
        });
        Ok(())
    }

    /// Opens (or renews) the guests' door at these addresses; both `None`
    /// closes it. The API has checked they're the box's own.
    pub fn set_door(
        &self,
        name: &str,
        v4: Option<Ipv4Addr>,
        v6: Option<Ipv6Addr>,
        now: u64,
    ) -> Result<(), Refused> {
        let until = if v4.is_none() && v6.is_none() {
            0
        } else {
            now + DOOR_LIFETIME
        };
        self.update(
            "door_v4 = ?1, door_v6 = ?2, door_until = ?3",
            params![
                v4.map(|a| a.to_string()),
                v6.map(|a| a.to_string()),
                until,
                name
            ],
        )?;
        self.put_entry(name, |e| {
            e.door_v4 = v4;
            e.door_v6 = v6;
            e.door_until = until;
        });
        Ok(())
    }

    fn update(&self, set: &str, params: &[&dyn rusqlite::ToSql]) -> Result<(), Refused> {
        let db = self.db()?;
        let n = db
            .execute(
                &format!(
                    "UPDATE boxes SET {set} WHERE name = ?{} AND disabled = 0 AND stolen = 0",
                    params.len()
                ),
                params,
            )
            .map_err(Refused::internal)?;
        if n == 0 {
            return Err(not_found());
        }
        Ok(())
    }

    /// Gives up a name. It waits before anyone else can take it; the same box
    /// can take it back meanwhile.
    pub fn release(&self, name: &str, now: u64, hold_days: u32) -> Result<(), Refused> {
        let mut db = self.db()?;
        let tx = db.transaction().map_err(Refused::internal)?;
        let Some(row) = find(&tx, "name", name).map_err(Refused::internal)? else {
            return Err(not_found());
        };
        let key = row.box_key.to_z32();
        tx.execute("DELETE FROM boxes WHERE name = ?1", params![name])
            .map_err(Refused::internal)?;
        tx.execute(
            "DELETE FROM address_records WHERE box_key = ?1",
            params![key],
        )
        .map_err(Refused::internal)?;
        tx.execute(
            "INSERT OR REPLACE INTO released (name, box_key, until) VALUES (?1, ?2, ?3)",
            params![name, key, now + u64::from(hold_days) * 86_400],
        )
        .map_err(Refused::internal)?;
        event(&tx, &key, BoxState::Gone).map_err(Refused::internal)?;
        tx.commit().map_err(Refused::internal)?;
        if let Ok(mut z) = self.zone.write() {
            z.remove(name);
        }
        if let Ok(mut r) = self.records.write() {
            r.remove(&row.box_key);
        }
        self.bump();
        Ok(())
    }

    /// Stores a box's address record, already checked to be signed by its key
    /// and newer than the last. Only registered boxes may publish.
    pub fn put_record(&self, key: &PublicKey, payload: &[u8]) -> Result<(), Refused> {
        let row = self.find_by_key(key)?;
        if !row.as_ref().is_some_and(|r| !r.stolen) {
            return Err(Refused::new(
                403,
                "unknown_box",
                "This box isn't registered here.",
            ));
        }
        let db = self.db()?;
        db.execute(
            "INSERT OR REPLACE INTO address_records (box_key, payload) VALUES (?1, ?2)",
            params![key.to_z32(), payload],
        )
        .map_err(Refused::internal)?;
        drop(db);
        if let Ok(mut r) = self.records.write() {
            r.insert(*key, payload.to_vec());
        }
        self.bump();
        Ok(())
    }

    /// What changed for relays since `after`. Never mentions devices.
    pub fn feed(&self, after: u64) -> Result<BoxFeed, Refused> {
        let db = self.db()?;
        let (min, max): (Option<i64>, Option<i64>) = db
            .query_row("SELECT MIN(seq), MAX(seq) FROM box_events", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(Refused::internal)?;
        let max = max.unwrap_or(0) as u64;
        let min = min.unwrap_or(1) as u64;
        if after == 0 || after + 1 < min || after > max {
            let mut q = db
                .prepare(
                    "SELECT box_key, factory, stolen FROM boxes b
                     WHERE (b.account_id IS NULL OR b.account_id NOT IN (SELECT id FROM accounts WHERE blocked = 1))",
                )
                .map_err(Refused::internal)?;
            let changes = q
                .query_map([], |r| {
                    let state = if r.get::<_, bool>(2)? {
                        BoxState::Stolen
                    } else if r.get::<_, bool>(1)? {
                        BoxState::ActiveFactory
                    } else {
                        BoxState::Active
                    };
                    Ok(BoxChange {
                        key: Key(key_col(&r.get::<_, String>(0)?)?),
                        state,
                    })
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(Refused::internal)?;
            return Ok(BoxFeed {
                seq: max,
                full: true,
                changes,
            });
        }
        let mut q = db
            .prepare(
                "SELECT seq, box_key, state FROM box_events WHERE seq > ?1 ORDER BY seq LIMIT ?2",
            )
            .map_err(Refused::internal)?;
        let mut seq = after;
        let changes = q
            .query_map(params![after, FEED_PAGE], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    key_col(&r.get::<_, String>(1)?)?,
                    state_from(&r.get::<_, String>(2)?),
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(Refused::internal)?
            .into_iter()
            .map(|(s, key, state)| {
                seq = s as u64;
                BoxChange {
                    key: Key(key),
                    state,
                }
            })
            .collect();
        Ok(BoxFeed {
            seq,
            full: false,
            changes,
        })
    }

    /// Someone reports a name.
    pub fn report(&self, name: &str, reason: &str, from: IpAddr, now: u64) -> Result<(), Refused> {
        let db = self.db()?;
        db.execute(
            "INSERT INTO reports (name, reason, at, addr) VALUES (?1, ?2, ?3, ?4)",
            params![name, reason, now, address_bucket(from)],
        )
        .map_err(Refused::internal)?;
        Ok(())
    }

    /// Moves a name from `old_key`'s box to a new box, for an admin whose note
    /// the old box signed. The old key stops working everywhere.
    pub fn move_name(
        &self,
        old_key: &PublicKey,
        new_key: &PublicKey,
        new_dns_key: &PublicKey,
    ) -> Result<String, Refused> {
        let mut db = self.db()?;
        let tx = db.transaction().map_err(Refused::internal)?;
        let Some(row) = find(&tx, "box_key", &old_key.to_z32()).map_err(Refused::internal)? else {
            return Err(Refused::new(
                404,
                "not_found",
                "The old box has no name here any more.",
            ));
        };
        if row.disabled {
            return Err(not_found());
        }
        if find(&tx, "box_key", &new_key.to_z32())
            .map_err(Refused::internal)?
            .is_some()
        {
            return Err(Refused::new(
                409,
                "box_has_name",
                "The new box already has a name. Give it up first.",
            ));
        }
        tx.execute(
            "UPDATE boxes SET box_key = ?1, dns_update_key = ?2, stolen = 0,
                    door_v4 = NULL, door_v6 = NULL, door_until = 0,
                    acme_box = '[]', acme_door = '[]', acme_until = 0
             WHERE name = ?3",
            params![new_key.to_z32(), new_dns_key.to_z32(), row.name],
        )
        .map_err(Refused::internal)?;
        tx.execute(
            "DELETE FROM address_records WHERE box_key = ?1",
            params![old_key.to_z32()],
        )
        .map_err(Refused::internal)?;
        event(&tx, &old_key.to_z32(), BoxState::Gone).map_err(Refused::internal)?;
        let state = if row.factory {
            BoxState::ActiveFactory
        } else {
            BoxState::Active
        };
        event(&tx, &new_key.to_z32(), state).map_err(Refused::internal)?;
        tx.commit().map_err(Refused::internal)?;
        drop(db);
        self.reload().map_err(Refused::internal)?;
        Ok(row.name)
    }

    /// Marks the box with this key stolen, for an admin whose note it signed.
    pub fn stolen_by_key(&self, key: &PublicKey) -> Result<String, Refused> {
        let Some(row) = self.find_by_key(key)? else {
            return Err(not_found());
        };
        self.set_stolen(&row.name).map_err(Refused::internal)?;
        Ok(row.name)
    }

    /// Everything DNS answers, for the second DNS server.
    pub fn zone_copy(&self) -> zoomriya_proto::ZoneCopy {
        let names = self
            .zone
            .read()
            .map(|z| {
                let mut v: Vec<_> = z
                    .iter()
                    .map(|(name, e)| zoomriya_proto::ZoneName {
                        name: name.clone(),
                        home: e.home.clone(),
                        door_v4: e.door_v4,
                        door_v6: e.door_v6,
                        door_until: e.door_until,
                        acme_box: e.acme_box.clone(),
                        acme_door: e.acme_door.clone(),
                        acme_until: e.acme_until,
                    })
                    .collect();
                v.sort_by(|a, b| a.name.cmp(&b.name));
                v
            })
            .unwrap_or_default();
        let records = self
            .records
            .read()
            .map(|r| {
                r.iter()
                    .map(|(k, p)| (Key(*k), data_encoding::BASE64URL_NOPAD.encode(p)))
                    .collect()
            })
            .unwrap_or_default();
        zoomriya_proto::ZoneCopy {
            serial: self.serial(),
            names,
            records,
        }
    }

    /// Replaces what DNS answers with a copy from the first server. Only for
    /// the second DNS server, which keeps nothing in its database.
    pub fn take_zone_copy(&self, copy: &zoomriya_proto::ZoneCopy) {
        let zone = copy
            .names
            .iter()
            .map(|n| {
                (
                    n.name.clone(),
                    Entry {
                        home: n.home.clone(),
                        door_v4: n.door_v4,
                        door_v6: n.door_v6,
                        door_until: n.door_until,
                        acme_box: n.acme_box.clone(),
                        acme_door: n.acme_door.clone(),
                        acme_until: n.acme_until,
                    },
                )
            })
            .collect();
        let records = copy
            .records
            .iter()
            .filter_map(|(k, p)| {
                data_encoding::BASE64URL_NOPAD
                    .decode(p.as_bytes())
                    .ok()
                    .map(|p| (k.0, p))
            })
            .collect();
        if let Ok(mut z) = self.zone.write() {
            *z = zone;
        }
        if let Ok(mut r) = self.records.write() {
            *r = records;
        }
        self.serial.store(copy.serial, Ordering::Relaxed);
    }

    /// Boxes for the GUI, newest first, optionally only names containing `q`.
    pub fn list_boxes(&self, q: &str, limit: u32) -> Result<Vec<BoxSummary>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut st = db.prepare(
            "SELECT b.name, b.box_key, b.factory, b.stolen, b.disabled, b.registered_at,
                    b.door_until, a.box_key IS NOT NULL, COALESCE(u.bytes, 0), al.gb, ac.username
             FROM boxes b LEFT JOIN address_records a ON a.box_key = b.box_key
             LEFT JOIN relay_use u ON u.box_key = b.box_key AND u.month = ?3
             LEFT JOIN allowances al ON al.box_key = b.box_key
             LEFT JOIN accounts ac ON ac.id = b.account_id
             WHERE instr(b.name, ?1) > 0 OR instr(COALESCE(ac.username, ''), ?1) > 0
             ORDER BY b.registered_at DESC LIMIT ?2",
        )?;
        let month = zoomriya_proto::month_of(zoomriya_serve::now());
        let rows = st
            .query_map(params![q.to_ascii_lowercase(), limit, month], |r| {
                Ok(BoxSummary {
                    name: r.get(0)?,
                    key: r.get(1)?,
                    factory: r.get(2)?,
                    stolen: r.get(3)?,
                    disabled: r.get(4)?,
                    registered_at: r.get(5)?,
                    door_until: r.get(6)?,
                    has_record: r.get(7)?,
                    used: r.get::<_, i64>(8)?.max(0) as u64,
                    own_allowance_gb: r.get(9)?,
                    account: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// New boxes since `since`.
    pub fn registered_since(&self, since: u64) -> Result<u64> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db.query_row(
            "SELECT COUNT(*) FROM boxes WHERE registered_at > ?1",
            params![since],
            |r| r.get(0),
        )?)
    }

    /// Factory tokens: not yet used, and used.
    pub fn factory_token_counts(&self) -> Result<(u64, u64)> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db.query_row(
            "SELECT COALESCE(SUM(used_by IS NULL),0), COALESCE(SUM(used_by IS NOT NULL),0) FROM factory_tokens",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    // Operator commands. They run in their own process; the server reloads.

    /// Adds factory tokens (only their hashes are kept). Returns how many were new.
    pub fn import_factory_tokens<'a>(
        &self,
        tokens: impl IntoIterator<Item = &'a str>,
    ) -> Result<usize> {
        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let tx = db.transaction()?;
        let mut n = 0;
        for t in tokens {
            let t = t.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            anyhow::ensure!(
                t.len() >= 20,
                "factory tokens must be at least 20 characters"
            );
            n += tx.execute(
                "INSERT OR IGNORE INTO factory_tokens (sha256) VALUES (?1)",
                params![token_hash(t)],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Switches a name off (after a report), or on again.
    pub fn set_disabled(&self, name: &str, disabled: bool) -> Result<bool> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let n = db.execute(
            "UPDATE boxes SET disabled = ?1 WHERE name = ?2",
            params![disabled, name],
        )?;
        drop(db);
        self.reload()?;
        Ok(n > 0)
    }

    /// Marks a box stolen: the relays refuse it and its devices, and its name
    /// stops answering.
    pub fn set_stolen(&self, name: &str) -> Result<bool> {
        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let tx = db.transaction()?;
        let Some(row) = find(&tx, "name", name)? else {
            return Ok(false);
        };
        tx.execute("UPDATE boxes SET stolen = 1 WHERE name = ?1", params![name])?;
        event(&tx, &row.box_key.to_z32(), BoxState::Stolen)?;
        tx.commit()?;
        drop(db);
        self.reload()?;
        Ok(true)
    }

    /// Reports not yet dealt with: id, name, reason, when.
    pub fn open_reports(&self) -> Result<Vec<(i64, String, String, u64)>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut q =
            db.prepare("SELECT id, name, reason, at FROM reports WHERE handled = 0 ORDER BY id")?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Marks a report dealt with.
    pub fn close_report(&self, id: i64) -> Result<bool> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db.execute("UPDATE reports SET handled = 1 WHERE id = ?1", params![id])? > 0)
    }

    /// The allowances, as last set in the GUI.
    pub fn allowances(&self) -> Result<Allowances> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut a = Allowances::default();
        let mut q = db.prepare("SELECT name, value FROM settings")?;
        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (name, value) = row?;
            let v = value.clamp(0, i64::from(u32::MAX)) as u32;
            match name.as_str() {
                "default_gb" => a.default_gb = v,
                "factory_gb" => a.factory_gb = v,
                "slow_kbps" => a.slow_kbps = v.max(1),
                _ => {}
            }
        }
        Ok(a)
    }

    /// Sets the allowances.
    pub fn set_allowances(&self, a: &Allowances) -> Result<()> {
        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let tx = db.transaction()?;
        for (name, v) in [
            ("default_gb", a.default_gb),
            ("factory_gb", a.factory_gb),
            ("slow_kbps", a.slow_kbps.max(1)),
        ] {
            tx.execute(
                "INSERT INTO settings (name, value) VALUES (?1, ?2)
                 ON CONFLICT (name) DO UPDATE SET value = excluded.value",
                params![name, v],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Gives one box its own allowance in GB (0: no limit), or `None` for the
    /// usual one. Returns `false` if there's no such box.
    pub fn set_box_allowance(&self, name: &str, gb: Option<u32>) -> Result<bool> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let Some(key): Option<String> = db
            .query_row(
                "SELECT box_key FROM boxes WHERE name = ?1",
                params![name],
                |r| r.get(0),
            )
            .optional()?
        else {
            return Ok(false);
        };
        match gb {
            Some(gb) => db.execute(
                "INSERT INTO allowances (box_key, gb) VALUES (?1, ?2)
                 ON CONFLICT (box_key) DO UPDATE SET gb = excluded.gb",
                params![key, gb],
            )?,
            None => db.execute("DELETE FROM allowances WHERE box_key = ?1", params![key])?,
        };
        Ok(true)
    }

    /// Adds a relay's counts to the month. Unknown boxes are skipped.
    pub fn add_relay_use(&self, month: &str, used: &[(PublicKey, u64)]) -> Result<()> {
        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let tx = db.transaction()?;
        for (key, bytes) in used {
            let bytes = i64::try_from(*bytes).unwrap_or(i64::MAX);
            tx.execute(
                "INSERT INTO relay_use (box_key, month, bytes)
                 SELECT box_key, ?2, ?3 FROM boxes WHERE box_key = ?1
                 ON CONFLICT (box_key, month) DO UPDATE SET bytes = bytes + excluded.bytes",
                params![key.to_z32(), month, bytes],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Boxes past their allowance this month.
    pub fn over_allowance(&self, month: &str) -> Result<Vec<PublicKey>> {
        let a = self.allowances()?;
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut q = db.prepare(
            "SELECT u.box_key FROM relay_use u
             JOIN boxes b ON b.box_key = u.box_key
             LEFT JOIN allowances al ON al.box_key = u.box_key
             WHERE u.month = ?1
               AND COALESCE(al.gb, CASE WHEN b.factory THEN ?3 ELSE ?2 END) > 0
               AND u.bytes > COALESCE(al.gb, CASE WHEN b.factory THEN ?3 ELSE ?2 END) * ?4",
        )?;
        let keys = q
            .query_map(params![month, a.default_gb, a.factory_gb, GB as i64], |r| {
                key_col(&r.get::<_, String>(0)?)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(keys)
    }

    /// One box's use this month: bytes, and its allowance in bytes (0: none).
    pub fn relay_use(&self, key: &PublicKey, month: &str) -> Result<(u64, u64)> {
        let a = self.allowances()?;
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let (factory, own, used): (bool, Option<i64>, i64) = db.query_row(
            "SELECT b.factory, al.gb, COALESCE(u.bytes, 0) FROM boxes b
             LEFT JOIN allowances al ON al.box_key = b.box_key
             LEFT JOIN relay_use u ON u.box_key = b.box_key AND u.month = ?2
             WHERE b.box_key = ?1",
            params![key.to_z32(), month],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let gb = own.map_or(if factory { a.factory_gb } else { a.default_gb }, |g| {
            g.clamp(0, i64::from(u32::MAX)) as u32
        });
        Ok((used.max(0) as u64, u64::from(gb) * GB))
    }

    /// Bytes through our relays this month, all boxes together.
    pub fn relay_use_total(&self, month: &str) -> Result<u64> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let n: i64 = db.query_row(
            "SELECT COALESCE(SUM(bytes), 0) FROM relay_use WHERE month = ?1",
            params![month],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    /// Names for box keys, for the GUI's list of connected boxes.
    pub fn names_for(&self, keys: &[PublicKey]) -> Result<HashMap<PublicKey, String>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut q = db.prepare("SELECT name FROM boxes WHERE box_key = ?1")?;
        let mut out = HashMap::new();
        for k in keys {
            if let Some(name) = q
                .query_row(params![k.to_z32()], |r| r.get::<_, String>(0))
                .optional()?
            {
                out.insert(*k, name);
            }
        }
        Ok(out)
    }

    // Accounts. Operators issue them; families change their password.

    /// Makes an account. `password` is already hashed.
    pub fn create_account(
        &self,
        username: &str,
        password: &str,
        max_boxes: u32,
        now: u64,
    ) -> Result<bool> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let n = db.execute(
            "INSERT OR IGNORE INTO accounts (username, password, max_boxes, created_at, password_set_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![username, password, max_boxes, now],
        )?;
        Ok(n > 0)
    }

    /// An account, for its sign-in.
    pub fn account(&self, username: &str) -> Result<Option<AccountRow>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db
            .query_row(
                "SELECT id, username, password, blocked, locked_until FROM accounts WHERE username = ?1",
                params![username],
                |r| {
                    Ok(AccountRow {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        password: r.get(2)?,
                        blocked: r.get(3)?,
                        locked_until: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// A wrong password: after [`WRONG_PASSWORDS`], the account is locked
    /// for [`LOCKED_FOR`].
    pub fn wrong_password(&self, id: i64, now: u64) -> Result<()> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        db.execute(
            "UPDATE accounts SET
                locked_until = CASE WHEN failed + 1 >= ?2 THEN ?3 ELSE locked_until END,
                failed = CASE WHEN failed + 1 >= ?2 THEN 0 ELSE failed + 1 END
             WHERE id = ?1",
            params![id, WRONG_PASSWORDS, now + LOCKED_FOR],
        )?;
        Ok(())
    }

    /// A right password: the count starts again.
    pub fn right_password(&self, id: i64) -> Result<()> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        db.execute(
            "UPDATE accounts SET failed = 0, locked_until = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Sets a new password (already hashed) and ends every session.
    pub fn set_password(&self, username: &str, password: &str, now: u64) -> Result<bool> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let n = db.execute(
            "UPDATE accounts SET password = ?2, password_set_at = ?3, failed = 0, locked_until = 0
             WHERE username = ?1",
            params![username, password, now],
        )?;
        db.execute(
            "DELETE FROM account_sessions WHERE account_id = (SELECT id FROM accounts WHERE username = ?1)",
            params![username],
        )?;
        Ok(n > 0)
    }

    /// Blocks an account, or unblocks it. Blocked, its boxes drop out of DNS
    /// and the relays refuse them and their devices; its sessions end.
    pub fn set_account_blocked(&self, username: &str, blocked: bool) -> Result<bool> {
        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let tx = db.transaction()?;
        let Some(id): Option<i64> = tx
            .query_row(
                "SELECT id FROM accounts WHERE username = ?1",
                params![username],
                |r| r.get(0),
            )
            .optional()?
        else {
            return Ok(false);
        };
        tx.execute(
            "UPDATE accounts SET blocked = ?2 WHERE id = ?1",
            params![id, blocked],
        )?;
        if blocked {
            tx.execute(
                "DELETE FROM account_sessions WHERE account_id = ?1",
                params![id],
            )?;
        }
        let boxes: Vec<(String, bool, bool)> = {
            let mut q =
                tx.prepare("SELECT box_key, factory, stolen FROM boxes WHERE account_id = ?1")?;
            q.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (key, factory, stolen) in boxes {
            let state = if blocked {
                BoxState::Gone
            } else if stolen {
                BoxState::Stolen
            } else if factory {
                BoxState::ActiveFactory
            } else {
                BoxState::Active
            };
            event(&tx, &key, state)?;
        }
        tx.commit()?;
        drop(db);
        self.reload()?;
        Ok(true)
    }

    /// Sets how many boxes an account may link.
    pub fn set_max_boxes(&self, username: &str, max: u32) -> Result<bool> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db.execute(
            "UPDATE accounts SET max_boxes = ?2 WHERE username = ?1",
            params![username, max],
        )? > 0)
    }

    /// Accounts for the GUI, optionally only usernames containing `q`.
    pub fn list_accounts(&self, q: &str, limit: u32) -> Result<Vec<AccountSummary>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut st = db.prepare(
            "SELECT a.username, a.blocked, a.max_boxes, a.created_at, a.locked_until,
                    COALESCE((SELECT group_concat(name, ' ') FROM boxes WHERE account_id = a.id), '')
             FROM accounts a WHERE instr(a.username, ?1) > 0
             ORDER BY a.created_at DESC LIMIT ?2",
        )?;
        let rows = st
            .query_map(params![q.to_lowercase(), limit], |r| {
                let boxes: String = r.get(5)?;
                Ok(AccountSummary {
                    username: r.get(0)?,
                    blocked: r.get(1)?,
                    max_boxes: r.get(2)?,
                    created_at: r.get(3)?,
                    locked_until: r.get(4)?,
                    boxes: boxes.split_whitespace().map(String::from).collect(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// An account's boxes: name, box key, ready-made.
    pub fn account_boxes(&self, id: i64) -> Result<Vec<(String, PublicKey, bool)>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut q = db.prepare(
            "SELECT name, box_key, factory FROM boxes WHERE account_id = ?1 ORDER BY name",
        )?;
        let rows = q
            .query_map(params![id], |r| {
                Ok((r.get(0)?, key_col(&r.get::<_, String>(1)?)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Accounts: all, and blocked.
    pub fn account_counts(&self) -> Result<(u64, u64)> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db.query_row(
            "SELECT COUNT(*), COALESCE(SUM(blocked), 0) FROM accounts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Keeps a sign-in session for the account page.
    pub fn new_session(&self, id: i64, token_hash: &str, expires: u64, now: u64) -> Result<()> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        db.execute(
            "DELETE FROM account_sessions WHERE expires <= ?1",
            params![now],
        )?;
        db.execute(
            "INSERT INTO account_sessions (token_hash, account_id, expires) VALUES (?1, ?2, ?3)",
            params![token_hash, id, expires],
        )?;
        Ok(())
    }

    /// The account a session belongs to, while it lasts: number, username,
    /// how many boxes it may link.
    pub fn session(&self, token_hash: &str, now: u64) -> Result<Option<(i64, String, u32)>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db
            .query_row(
                "SELECT a.id, a.username, a.max_boxes FROM account_sessions s
                 JOIN accounts a ON a.id = s.account_id
                 WHERE s.token_hash = ?1 AND s.expires > ?2 AND a.blocked = 0",
                params![token_hash, now],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }

    /// Ends a session.
    pub fn end_session(&self, token_hash: &str) -> Result<()> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        db.execute(
            "DELETE FROM account_sessions WHERE token_hash = ?1",
            params![token_hash],
        )?;
        Ok(())
    }

    /// How many boxes are registered: all, with a factory token, stolen, switched off.
    pub fn counts(&self) -> Result<(u64, u64, u64, u64)> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(db.query_row(
            "SELECT COUNT(*), COALESCE(SUM(factory),0), COALESCE(SUM(stolen),0), COALESCE(SUM(disabled),0) FROM boxes",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?)
    }
}

fn account_needed() -> Refused {
    Refused::new(
        403,
        "account_needed",
        "ZOOMRIYA Connect needs an account. Type the username and password you were given.",
    )
}

/// The account's username, if it isn't blocked.
fn account_usable(db: &Connection, id: i64) -> Result<String, Refused> {
    let (username, blocked): (String, bool) = db
        .query_row(
            "SELECT username, blocked FROM accounts WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(Refused::internal)?
        .ok_or_else(account_needed)?;
    if blocked {
        return Err(Refused::new(
            403,
            "account_blocked",
            "This ZOOMRIYA Connect account is blocked. Ask whoever gave you the account.",
        ));
    }
    Ok(username)
}

/// Refuses a box past the account's limit.
fn account_has_room(db: &Connection, id: i64) -> Result<(), Refused> {
    let (count, max): (u32, u32) = db
        .query_row(
            "SELECT (SELECT COUNT(*) FROM boxes WHERE account_id = ?1), max_boxes
             FROM accounts WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(Refused::internal)?
        .ok_or_else(account_needed)?;
    if count >= max {
        return Err(Refused::new(
            403,
            "account_full",
            "This account already has all the boxes it can. Ask whoever gave you the account for room for another.",
        ));
    }
    Ok(())
}

fn name_taken() -> Refused {
    Refused::new(409, "name_taken", "That name is taken. Pick another.")
}

fn not_found() -> Refused {
    Refused::new(404, "not_found", "There's no box with that name here.")
}

fn check_home(home: &HomeAddrs) -> Result<(), Refused> {
    let all = home
        .ipv4
        .iter()
        .map(|a| IpAddr::V4(*a))
        .chain(home.ipv6.iter().map(|a| IpAddr::V6(*a)));
    if home.ipv4.len() + home.ipv6.len() > 8 || !all.into_iter().all(zoomriya_proto::addrs::is_home)
    {
        return Err(Refused::new(
            400,
            "not_home_address",
            "A box name can only point at the box on your home network.",
        ));
    }
    Ok(())
}

fn find(db: &Connection, col: &str, value: &str) -> rusqlite::Result<Option<BoxRow>> {
    db.query_row(
        &format!(
            "SELECT name, box_key, dns_update_key, factory, stolen, disabled FROM boxes WHERE {col} = ?1"
        ),
        params![value],
        |r| {
            Ok(BoxRow {
                name: r.get(0)?,
                box_key: key_col(&r.get::<_, String>(1)?)?,
                dns_update_key: key_col(&r.get::<_, String>(2)?)?,
                factory: r.get(3)?,
                stolen: r.get(4)?,
                disabled: r.get(5)?,
            })
        },
    )
    .optional()
}

fn event(db: &Connection, box_key: &str, state: BoxState) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO box_events (box_key, state) VALUES (?1, ?2)",
        params![box_key, state_str(state)],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn entry_from_row(
    home: &str,
    door_v4: Option<String>,
    door_v6: Option<String>,
    door_until: u64,
    acme_box: &str,
    acme_door: &str,
    acme_until: u64,
) -> Entry {
    Entry {
        home: serde_json::from_str(home).unwrap_or_default(),
        door_v4: door_v4.and_then(|s| s.parse().ok()),
        door_v6: door_v6.and_then(|s| s.parse().ok()),
        door_until,
        acme_box: serde_json::from_str(acme_box).unwrap_or_default(),
        acme_door: serde_json::from_str(acme_door).unwrap_or_default(),
        acme_until,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoomriya_keys::SecretKey;

    const NOW: u64 = 1_800_000_000;

    fn rules() -> Rules {
        Rules {
            names: NameRules::new(),
            per_address_per_day: 2,
            released_name_days: 90,
            accounts_required: false,
        }
    }

    fn req(name: &str, key: &SecretKey) -> RegisterRequest {
        RegisterRequest {
            name: name.into(),
            box_key: key.public().into(),
            dns_update_key: SecretKey::generate().public().into(),
            home: HomeAddrs {
                ipv4: vec!["192.168.1.20".parse().expect("ip")],
                ipv6: vec![],
            },
            factory_token: None,
            account: None,
        }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("ip")
    }

    #[test]
    fn registering() {
        let s = Store::in_memory().expect("store");
        let a = SecretKey::generate();
        let r = s
            .register(&req("Maple", &a), ip("81.2.69.160"), NOW, &rules(), None)
            .expect("ok");
        assert_eq!(r.name, "maple");
        assert!(r.new);
        assert_eq!(s.entry("maple").expect("in DNS").home.ipv4.len(), 1);

        // Same box again: fine, takes the new DNS key, doesn't count.
        let again = req("maple", &a);
        let r = s
            .register(&again, ip("81.2.69.160"), NOW, &rules(), None)
            .expect("again");
        assert!(!r.new);
        assert_eq!(
            s.find("maple").expect("db").expect("row").dns_update_key,
            again.dns_update_key.0
        );

        // Someone else wants it.
        let b = SecretKey::generate();
        let e = s
            .register(&req("maple", &b), ip("81.2.69.161"), NOW, &rules(), None)
            .expect_err("taken");
        assert_eq!(e.code, "name_taken");
        // One name per box.
        let e = s
            .register(&req("oak", &a), ip("81.2.69.160"), NOW, &rules(), None)
            .expect_err("one");
        assert_eq!(e.code, "box_has_name");
        // A name pointing at the internet.
        let mut bad = req("elm", &b);
        bad.home.ipv4 = vec!["8.8.8.8".parse().expect("ip")];
        assert_eq!(
            s.register(&bad, ip("81.2.69.161"), NOW, &rules(), None)
                .expect_err("public")
                .code,
            "not_home_address"
        );
        // Name rules.
        assert_eq!(
            s.register(&req("mybank", &b), ip("81.2.69.161"), NOW, &rules(), None)
                .expect_err("blocked")
                .code,
            "name_blocked"
        );
    }

    #[test]
    fn daily_limit_and_factory_tokens() {
        let s = Store::in_memory().expect("store");
        let from = ip("2a01:4f8:1:2::5");
        for n in ["one", "two"] {
            s.register(&req(n, &SecretKey::generate()), from, NOW, &rules(), None)
                .expect("ok");
        }
        // Same /64, another address.
        let e = s
            .register(
                &req("three", &SecretKey::generate()),
                ip("2a01:4f8:1:2::99"),
                NOW,
                &rules(),
                None,
            )
            .expect_err("limit");
        assert_eq!(e.code, "too_many_boxes");
        assert_eq!(e.status, 429);
        // A day later.
        s.register(
            &req("three", &SecretKey::generate()),
            from,
            NOW + 86_401,
            &rules(),
            None,
        )
        .expect("next day");

        // A factory token skips the limit, once.
        assert_eq!(
            s.import_factory_tokens(["# batch 1", "tok-aaaaaaaaaaaaaaaaaaaa"])
                .expect("import"),
            1
        );
        let mut f = req("four", &SecretKey::generate());
        f.factory_token = Some("tok-aaaaaaaaaaaaaaaaaaaa".into());
        assert!(
            s.register(&f, from, NOW, &rules(), None)
                .expect("factory")
                .factory
        );
        let mut copied = req("five", &SecretKey::generate());
        copied.factory_token = Some("tok-aaaaaaaaaaaaaaaaaaaa".into());
        assert_eq!(
            s.register(&copied, from, NOW, &rules(), None)
                .expect_err("used")
                .code,
            "factory_token"
        );
    }

    #[test]
    fn releasing_and_waiting() {
        let s = Store::in_memory().expect("store");
        let a = SecretKey::generate();
        let b = SecretKey::generate();
        s.register(&req("maple", &a), ip("81.2.69.160"), NOW, &rules(), None)
            .expect("ok");
        s.release("maple", NOW, 90).expect("release");
        assert!(s.entry("maple").is_none());
        assert_eq!(
            s.register(
                &req("maple", &b),
                ip("81.2.69.1"),
                NOW + 86_400,
                &rules(),
                None
            )
            .expect_err("waiting")
            .code,
            "name_taken"
        );
        // The same box can take it back.
        s.register(
            &req("maple", &a),
            ip("81.2.69.160"),
            NOW + 86_400,
            &rules(),
            None,
        )
        .expect("back");
        s.release("maple", NOW + 86_400, 90).expect("release");
        // After 90 days anyone can.
        s.register(
            &req("maple", &b),
            ip("81.2.69.1"),
            NOW + 92 * 86_400,
            &rules(),
            None,
        )
        .expect("free");
    }

    #[test]
    fn feed_for_relays() {
        let s = Store::in_memory().expect("store");
        let full = s.feed(0).expect("feed");
        assert!(full.full);
        assert!(full.changes.is_empty());
        let a = SecretKey::generate();
        let b = SecretKey::generate();
        s.register(&req("maple", &a), ip("81.2.69.160"), NOW, &rules(), None)
            .expect("ok");
        // Asking from zero always gets the whole list.
        let f = s.feed(full.seq).expect("feed");
        assert!(f.full);
        assert_eq!(
            f.changes,
            vec![BoxChange {
                key: a.public().into(),
                state: BoxState::Active
            }]
        );
        s.register(&req("oak", &b), ip("81.2.69.161"), NOW, &rules(), None)
            .expect("ok");
        s.set_stolen("maple").expect("stolen");
        let g = s.feed(f.seq).expect("feed");
        assert_eq!(g.changes.len(), 2);
        assert_eq!(g.changes[1].state, BoxState::Stolen);
        assert!(
            s.entry("maple").is_none(),
            "stolen box's name stops answering"
        );
        // Nothing new.
        let h = s.feed(g.seq).expect("feed");
        assert!(h.changes.is_empty());
        assert_eq!(h.seq, g.seq);
        // From scratch.
        let all = s.feed(0).expect("feed");
        assert!(all.full);
        assert_eq!(all.changes.len(), 2);
    }

    #[test]
    fn door_acme_and_switching_off() {
        let s = Store::in_memory().expect("store");
        let a = SecretKey::generate();
        s.register(&req("maple", &a), ip("81.2.69.160"), NOW, &rules(), None)
            .expect("ok");
        s.set_door("maple", Some("81.2.69.160".parse().expect("ip")), None, NOW)
            .expect("door");
        s.set_acme("maple", AcmeTarget::Door, &["abc".into()], NOW)
            .expect("acme");
        let e = s.entry("maple").expect("entry");
        assert_eq!(e.door_until, NOW + DOOR_LIFETIME);
        assert_eq!(e.acme_door, vec!["abc".to_string()]);
        assert!(
            s.set_acme("maple", AcmeTarget::Box, &["x".repeat(256)], NOW)
                .is_err()
        );
        assert_eq!(
            s.set_door("nobody", None, None, NOW)
                .expect_err("none")
                .code,
            "not_found"
        );

        assert!(s.set_disabled("maple", true).expect("off"));
        assert!(s.entry("maple").is_none());
        assert_eq!(
            s.set_home("maple", &HomeAddrs::default())
                .expect_err("off")
                .code,
            "not_found"
        );
        assert!(s.set_disabled("maple", false).expect("on"));
        assert!(s.entry("maple").is_some());
    }

    #[test]
    fn moving_a_name() {
        let s = Store::in_memory().expect("store");
        let old = SecretKey::generate();
        let new = SecretKey::generate();
        let dns = SecretKey::generate();
        s.register(&req("maple", &old), ip("81.2.69.160"), NOW, &rules(), None)
            .expect("ok");
        let seq = s.feed(0).expect("feed").seq;
        assert_eq!(
            s.move_name(&old.public(), &new.public(), &dns.public())
                .expect("moves"),
            "maple"
        );
        let row = s.find("maple").expect("db").expect("row");
        assert_eq!(row.box_key, new.public());
        assert_eq!(row.dns_update_key, dns.public());
        let f = s.feed(seq).expect("feed");
        assert_eq!(
            f.changes,
            vec![
                BoxChange {
                    key: old.public().into(),
                    state: BoxState::Gone
                },
                BoxChange {
                    key: new.public().into(),
                    state: BoxState::Active
                },
            ]
        );
        // The old key can't move it again.
        assert_eq!(
            s.move_name(
                &old.public(),
                &SecretKey::generate().public(),
                &dns.public()
            )
            .expect_err("gone")
            .code,
            "not_found"
        );
        // Stolen by key.
        assert_eq!(s.stolen_by_key(&new.public()).expect("stolen"), "maple");
        assert!(s.find("maple").expect("db").expect("row").stolen);
    }

    #[test]
    fn zone_copies() {
        let s = Store::in_memory().expect("store");
        s.register(
            &req("maple", &SecretKey::generate()),
            ip("81.2.69.160"),
            NOW,
            &rules(),
            None,
        )
        .expect("ok");
        let copy = s.zone_copy();
        assert_eq!(copy.names.len(), 1);
        let second = Store::in_memory().expect("store");
        second.take_zone_copy(&copy);
        assert_eq!(second.entry("maple"), s.entry("maple"));
        assert_eq!(second.serial(), copy.serial);
        let listed = s.list_boxes("map", 10).expect("list");
        assert_eq!(listed.len(), 1);
        assert!(s.list_boxes("oak", 10).expect("list").is_empty());
    }

    #[test]
    fn address_buckets() {
        assert_eq!(address_bucket(ip("81.2.69.160")), "81.2.69.160");
        assert_eq!(address_bucket(ip("::ffff:81.2.69.160")), "81.2.69.160");
        assert_eq!(address_bucket(ip("2a01:4f8:1:2::5")), "2a01:4f8:1:2::/64");
    }

    #[test]
    fn monthly_allowance() {
        let s = Store::in_memory().expect("store");
        let a = SecretKey::generate();
        let b = SecretKey::generate();
        s.register(&req("maple", &a), ip("81.2.69.160"), NOW, &rules(), None)
            .expect("ok");
        s.register(&req("oak", &b), ip("81.2.69.161"), NOW, &rules(), None)
            .expect("ok");
        let stranger = SecretKey::generate().public();
        let month = "2027-01";

        // Counted per box and month; boxes we don't know are skipped.
        s.add_relay_use(month, &[(a.public(), 3 * GB), (stranger, GB)])
            .expect("adds");
        s.add_relay_use(month, &[(a.public(), 2 * GB), (b.public(), GB)])
            .expect("adds");
        s.add_relay_use("2026-12", &[(b.public(), 99 * GB)])
            .expect("adds");
        assert_eq!(s.relay_use_total(month).expect("total"), 6 * GB);

        // No limit until an operator sets one.
        assert_eq!(s.allowances().expect("read"), Allowances::default());
        assert!(s.over_allowance(month).expect("over").is_empty());
        assert_eq!(s.relay_use(&a.public(), month).expect("use"), (5 * GB, 0));

        s.set_allowances(&Allowances {
            default_gb: 4,
            factory_gb: 50,
            slow_kbps: 100,
        })
        .expect("set");
        assert_eq!(s.over_allowance(month).expect("over"), vec![a.public()]);
        assert_eq!(
            s.relay_use(&a.public(), month).expect("use"),
            (5 * GB, 4 * GB)
        );

        // One box gets its own: more, or none at all.
        assert!(s.set_box_allowance("maple", Some(10)).expect("set"));
        assert!(s.over_allowance(month).expect("over").is_empty());
        assert!(s.set_box_allowance("maple", Some(0)).expect("set"));
        assert!(s.over_allowance(month).expect("over").is_empty());
        assert_eq!(s.relay_use(&a.public(), month).expect("use"), (5 * GB, 0));
        assert!(s.set_box_allowance("maple", None).expect("back"));
        assert_eq!(s.over_allowance(month).expect("over"), vec![a.public()]);
        assert!(!s.set_box_allowance("nobody", Some(1)).expect("no box"));

        // The GUI's list shows this month's use and any own allowance.
        let list = s.list_boxes("", 10).expect("list");
        assert!(list.iter().all(|b| b.own_allowance_gb.is_none()));
    }
}
