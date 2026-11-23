//! The family's own name, with a real padlock (docs/direct-plus.md §11):
//! `home.<domain>` for the family, `share.home.<domain>` for guests. Set in
//! Box Desktop (Control Panel › Remote access); kept in `names.json`,
//! readable by connectd only.
//!
//! | Job | How | How often |
//! |---|---|---|
//! | The names' addresses | RFC 2136 update with the family's TSIG key: AAAA for the box's lasting IPv6 addresses, A only for an address that reaches the box (a port the router opened, one router) | When they change; TTL 60 s |
//! | The certificate | Let's Encrypt, proved through DNS: the TXT record by the same key, or through acme-dns | When missing, or under 30 days left |
//! | Strangers' certificates | The public certificate logs, for any certificate for the names that the box did not ask for | Once a day |
//!
//! The certificate goes to `certificate/connect.pem` in connectd's folder;
//! ZOOMRIYA OS's agent checks it and the gateway serves it
//! (zoomriya-os docs/design.md §8.1). Its key is made on the box and never
//! leaves it.

use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use hickory_proto::rr::RecordType;
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    daemon::Ctx,
    dns_update::{Tsig, Value},
};

/// Renew when the certificate has fewer days left than this.
pub const RENEW_DAYS: i64 = 30;
/// The records' cache time: short, so a new address spreads fast.
const TTL: u32 = 60;
/// How often the records and the certificate are looked at.
const LOOK: Duration = Duration::from_secs(60);
/// After a failed certificate, the wait before trying again.
const RETRY_CERT: u64 = 6 * 3600;
/// How often the certificate logs are read.
const WATCH: u64 = 24 * 3600;
/// How long the authority's check records get to spread.
const SPREAD: Duration = Duration::from_secs(30);

/// acme-dns: a server that holds only the certificate check's TXT record.
/// The family points `_acme-challenge.home.<domain>` and
/// `_acme-challenge.share.home.<domain>` at it with CNAME records, once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcmeDns {
    /// The server, such as `https://auth.example.org`.
    pub url: String,
    /// What it gave at registration.
    pub subdomain: String,
    /// What it gave at registration.
    pub username: String,
    /// What it gave at registration.
    pub password: String,
}

/// What Box Desktop sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The family's domain, such as `smith.ca`.
    pub domain: String,
    /// The key that changes its records.
    pub dns: Tsig,
    /// Or the certificate's check through acme-dns.
    #[serde(default)]
    pub acme_dns: Option<AcmeDns>,
    /// Let's Encrypt's test service: tests only.
    #[serde(default)]
    pub staging: bool,
}

impl Settings {
    /// The family's name.
    pub fn home(&self) -> String {
        format!("home.{}", self.domain)
    }

    /// The guests' name.
    pub fn share(&self) -> String {
        format!("share.home.{}", self.domain)
    }

    /// Checks the settings, without sending anything.
    pub fn check(&self) -> Result<()> {
        let d = self
            .domain
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        anyhow::ensure!(
            d.contains('.')
                && d.split('.').all(|l| {
                    !l.is_empty()
                        && l.len() <= 63
                        && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        && !l.starts_with('-')
                        && !l.ends_with('-')
                }),
            "{d} is not a domain"
        );
        anyhow::ensure!(
            d == self.dns.zone.trim_end_matches('.')
                || d.ends_with(&format!(".{}", self.dns.zone.trim_end_matches('.'))),
            "{d} is not in the zone {}",
            self.dns.zone
        );
        self.dns.check()?;
        if let Some(a) = &self.acme_dns {
            anyhow::ensure!(
                a.url.starts_with("https://"),
                "the acme-dns server needs https://"
            );
        }
        Ok(())
    }
}

/// What Box Desktop shows.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NamesStatus {
    /// The family's domain, when set.
    pub domain: Option<String>,
    /// `home.<domain>`.
    pub home: Option<String>,
    /// `share.home.<domain>`.
    pub share: Option<String>,
    /// The addresses the names have now.
    pub records: Vec<IpAddr>,
    /// When they were last changed.
    pub records_at: Option<u64>,
    /// Days left on the certificate.
    pub certificate_days: Option<i64>,
    /// The CAA record to add, so only this box's account can get
    /// certificates for the names.
    pub caa: Option<String>,
    /// What went wrong last, in plain words.
    pub problem: Option<String>,
    /// Certificates in the public logs that the box did not ask for.
    pub strangers: Vec<String>,
}

/// The family's name: settings and what happened.
#[derive(Debug)]
pub struct Names {
    file: PathBuf,
    settings: Mutex<Option<Settings>>,
    status: Mutex<NamesStatus>,
    wake: tokio::sync::Notify,
}

impl Names {
    /// Reads `file`; missing means none set.
    pub fn open(file: &Path) -> Result<Self> {
        let settings = match std::fs::read(file) {
            Ok(b) => Some(
                serde_json::from_slice(&b)
                    .with_context(|| format!("reading {}", file.display()))?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        Ok(Self {
            file: file.to_path_buf(),
            settings: Mutex::new(settings),
            status: Mutex::default(),
            wake: tokio::sync::Notify::new(),
        })
    }

    /// The settings, if set.
    pub fn settings(&self) -> Option<Settings> {
        self.settings.lock().ok().and_then(|s| s.clone())
    }

    /// Sets them (checked first), or forgets them with `None`.
    pub fn set(&self, s: Option<Settings>) -> Result<()> {
        match &s {
            Some(s) => {
                s.check()?;
                crate::files::write_private(&self.file, &serde_json::to_vec(s)?)?;
            }
            None => match std::fs::remove_file(&self.file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            },
        }
        if let Ok(mut now) = self.settings.lock() {
            *now = s;
        }
        if let Ok(mut st) = self.status.lock() {
            *st = NamesStatus::default();
        }
        self.wake.notify_one();
        Ok(())
    }

    /// What Box Desktop shows.
    pub fn status(&self) -> NamesStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut NamesStatus)) {
        if let Ok(mut s) = self.status.lock() {
            f(&mut s);
        }
    }
}

/// The addresses the names should have: only ones that reach the box.
pub fn addresses(reach: &crate::reach::Reach) -> (Vec<Value>, Vec<Value>) {
    let a = match (reach.port_mapped, reach.double_nat) {
        (Some(SocketAddr::V4(m)), Some(false)) => vec![Value::A(*m.ip())],
        _ => Vec::new(),
    };
    let aaaa = reach.ipv6.iter().map(|ip| Value::Aaaa(*ip)).collect();
    (a, aaaa)
}

/// Days left on the bundle's certificate, if there is one and it reads.
pub fn days_left(bundle: &Path, now: u64) -> Option<i64> {
    let pem = std::fs::read(bundle).ok()?;
    let (_, p) = x509_parser::pem::parse_x509_pem(&pem).ok()?;
    let cert = p.parse_x509().ok()?;
    let until = cert.validity().not_after.timestamp();
    Some((until - now as i64).div_euclid(86_400))
}

/// The serial number of the bundle's certificate, as the logs write it.
fn serial(bundle_pem: &str) -> Option<String> {
    let (_, p) = x509_parser::pem::parse_x509_pem(bundle_pem.as_bytes()).ok()?;
    let cert = p.parse_x509().ok()?;
    Some(
        cert.raw_serial_as_string()
            .replace(':', "")
            .to_ascii_lowercase(),
    )
}

/// Writes the bundle (the chain, then the key) beside `dir` first and moves
/// it in whole.
pub fn write_bundle(dir: &Path, chain_pem: &str, key_pem: &str) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    let mut bytes = chain_pem.trim_end().to_string();
    bytes.push('\n');
    bytes.push_str(key_pem.trim_end());
    bytes.push('\n');
    crate::files::write_private(&dir.join("connect.pem"), bytes.as_bytes())
}

/// The certificates the box asked for, by serial: so the logs' others stand out.
fn own_serials(data_dir: &Path) -> BTreeSet<String> {
    std::fs::read(data_dir.join("certificates.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn note_serial(data_dir: &Path, serial: String) -> Result<()> {
    let mut all = own_serials(data_dir);
    all.insert(serial);
    crate::files::write_private(
        &data_dir.join("certificates.json"),
        &serde_json::to_vec(&all)?,
    )
}

async fn account(s: &Settings, data_dir: &Path) -> Result<Account> {
    let file = data_dir.join(if s.staging {
        "acme-staging.json"
    } else {
        "acme.json"
    });
    if let Ok(bytes) = std::fs::read(&file) {
        let creds: AccountCredentials = serde_json::from_slice(&bytes)?;
        return Ok(Account::builder()?.from_credentials(creds).await?);
    }
    let url = if s.staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };
    let (account, creds) = Account::builder()?
        .create(
            &NewAccount {
                contact: &[],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            url.to_owned(),
            None,
        )
        .await
        .context("making an account with Let's Encrypt")?;
    crate::files::write_private(&file, &serde_json::to_vec(&creds)?)?;
    Ok(account)
}

/// The CAA record allowing only this box's account.
fn caa(account: &Account) -> String {
    format!("0 issue \"letsencrypt.org; accounturi={}\"", account.id())
}

async fn set_check(s: &Settings, name: &str, values: &[String]) -> Result<()> {
    if let Some(a) = &s.acme_dns {
        // acme-dns keeps the two newest values, so one each is enough.
        let client = reqwest::Client::new();
        for v in values {
            let r = client
                .post(format!("{}/update", a.url.trim_end_matches('/')))
                .header("X-Api-User", &a.username)
                .header("X-Api-Key", &a.password)
                .json(&serde_json::json!({ "subdomain": a.subdomain, "txt": v }))
                .send()
                .await
                .context("reaching the acme-dns server")?;
            anyhow::ensure!(
                r.status().is_success(),
                "the acme-dns server answered {}",
                r.status()
            );
        }
        return Ok(());
    }
    let values: Vec<Value> = values.iter().cloned().map(Value::Txt).collect();
    s.dns
        .replace(
            &format!("_acme-challenge.{name}"),
            RecordType::TXT,
            &values,
            TTL,
        )
        .await
}

/// Gets a certificate for both names and writes the bundle. Returns the
/// CAA record for the account.
async fn obtain(s: &Settings, data_dir: &Path) -> Result<String> {
    let account = account(s, data_dir).await?;
    let (home, share) = (s.home(), s.share());
    let ids = [
        Identifier::Dns(home.clone()),
        Identifier::Dns(share.clone()),
    ];
    let mut order = account.new_order(&NewOrder::new(&ids)).await?;
    let mut wanted: Vec<(String, String)> = Vec::new();
    {
        let mut auths = order.authorizations();
        while let Some(auth) = auths.next().await {
            let mut auth = auth?;
            if auth.status == AuthorizationStatus::Valid {
                continue;
            }
            let ch = auth
                .challenge(ChallengeType::Dns01)
                .context("Let's Encrypt offers no DNS check")?;
            let id = ch.identifier().to_string();
            anyhow::ensure!(id == home || id == share, "Let's Encrypt asked about {id}");
            wanted.push((id, ch.key_authorization().dns_value()));
        }
    }
    for name in [&home, &share] {
        let values: Vec<String> = wanted
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .collect();
        if !values.is_empty() {
            set_check(s, name, &values).await?;
        }
    }
    tokio::time::sleep(SPREAD).await;
    {
        let mut auths = order.authorizations();
        while let Some(auth) = auths.next().await {
            let mut auth = auth?;
            if auth.status == AuthorizationStatus::Valid {
                continue;
            }
            if let Some(mut ch) = auth.challenge(ChallengeType::Dns01) {
                ch.set_ready().await?;
            }
        }
    }
    let status = order.poll_ready(&RetryPolicy::default()).await?;
    // The check's records go, whatever happened.
    if s.acme_dns.is_none() {
        for name in [&home, &share] {
            let _ = s
                .dns
                .replace(
                    &format!("_acme-challenge.{name}"),
                    RecordType::TXT,
                    &[],
                    TTL,
                )
                .await;
        }
    }
    if status != OrderStatus::Ready {
        bail!("Let's Encrypt's check ended {status:?}: are the names' TXT records reachable?");
    }
    let key_pem = order.finalize().await?;
    let chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;
    if let Some(sn) = serial(&chain_pem) {
        note_serial(data_dir, sn)?;
    }
    write_bundle(&data_dir.join("certificate"), &chain_pem, &key_pem)?;
    Ok(caa(&account))
}

#[derive(Deserialize)]
struct LogEntry {
    serial_number: String,
    not_after: String,
    #[serde(default)]
    issuer_name: String,
}

/// Certificates for `names` in the public logs that the box did not ask
/// for, still valid. Through crt.sh, which reads the logs.
async fn strangers(names: &[String], own: &BTreeSet<String>, today: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let mut found = Vec::new();
    for name in names {
        let entries: Vec<LogEntry> = client
            .get("https://crt.sh/")
            .query(&[
                ("q", name.as_str()),
                ("output", "json"),
                ("exclude", "expired"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        for e in entries {
            let sn = e.serial_number.to_ascii_lowercase();
            if !own.contains(&sn) && e.not_after.as_str() >= today {
                found.push(format!("{name}: {} ({sn})", e.issuer_name));
            }
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn today(now: u64) -> String {
    // YYYY-MM-DD, as the logs write dates.
    let days = (now / 86_400) as i64;
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970 as a date (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Keeps the names' records and certificate current, until stopped.
pub async fn keep(ctx: Arc<Ctx>) {
    let names = ctx.names.clone();
    let data_dir = ctx.cfg.data_dir.clone();
    let bundle = data_dir.join("certificate").join("connect.pem");
    let mut last: Option<(Settings, Vec<Value>, Vec<Value>)> = None;
    let mut cert_after = 0u64;
    let mut watch_after = 0u64;
    loop {
        if let Some(s) = names.settings() {
            let now = crate::now();
            names.update(|st| {
                st.domain = Some(s.domain.clone());
                st.home = Some(s.home());
                st.share = Some(s.share());
                st.certificate_days = days_left(&bundle, now);
            });
            let reach = ctx.reach.lock().map(|r| r.clone()).unwrap_or_default();
            let (a, aaaa) = addresses(&reach);
            let changed = last
                .as_ref()
                .is_none_or(|(ls, la, laaaa)| *ls != s || *la != a || *laaaa != aaaa);
            if ctx.away.on()
                && changed
                && !(a.is_empty() && aaaa.is_empty() && reach.published_at.is_none())
            {
                match set_records(&s, &a, &aaaa).await {
                    Ok(()) => {
                        info!(home = %s.home(), "the names' addresses changed");
                        names.update(|st| {
                            st.records = a.iter().chain(&aaaa).map(ip).collect();
                            st.records_at = Some(now);
                            st.problem = None;
                        });
                        last = Some((s.clone(), a, aaaa));
                    }
                    Err(e) => {
                        warn!("the names' records: {e:#}");
                        names.update(|st| st.problem = Some(format!("{e:#}")));
                    }
                }
            }
            let days = days_left(&bundle, now);
            if days.is_none_or(|d| d < RENEW_DAYS) && now >= cert_after {
                match obtain(&s, &data_dir).await {
                    Ok(caa) => {
                        info!(home = %s.home(), "certificate");
                        names.update(|st| {
                            st.caa = Some(caa);
                            st.certificate_days = days_left(&bundle, crate::now());
                            st.problem = None;
                        });
                    }
                    Err(e) => {
                        warn!("the certificate: {e:#}");
                        cert_after = now + RETRY_CERT;
                        names.update(|st| st.problem = Some(format!("{e:#}")));
                    }
                }
            }
            if now >= watch_after && days.is_some() {
                watch_after = now + WATCH;
                match strangers(&[s.home(), s.share()], &own_serials(&data_dir), &today(now)).await
                {
                    Ok(found) => {
                        if !found.is_empty() {
                            warn!(?found, "certificates the box did not ask for");
                        }
                        names.update(|st| st.strangers = found);
                    }
                    Err(e) => warn!("reading the certificate logs: {e:#}"),
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(LOOK) => {}
            _ = names.wake.notified() => { last = None; cert_after = 0; }
        }
    }
}

fn ip(v: &Value) -> IpAddr {
    match v {
        Value::A(a) => IpAddr::V4(*a),
        Value::Aaaa(a) => IpAddr::V6(*a),
        Value::Txt(_) => IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
    }
}

async fn set_records(s: &Settings, a: &[Value], aaaa: &[Value]) -> Result<()> {
    for name in [s.home(), s.share()] {
        s.dns.replace(&name, RecordType::A, a, TTL).await?;
        s.dns.replace(&name, RecordType::AAAA, aaaa, TTL).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_encoding::BASE64;

    fn settings() -> Settings {
        Settings {
            domain: "smith.ca".into(),
            dns: Tsig {
                server: "ns1.smith.ca".into(),
                zone: "smith.ca".into(),
                key_name: "box".into(),
                algorithm: "hmac-sha256".into(),
                secret: BASE64.encode(&[1u8; 32]),
            },
            acme_dns: None,
            staging: false,
        }
    }

    #[test]
    fn names_and_checks() {
        let s = settings();
        assert_eq!(s.home(), "home.smith.ca");
        assert_eq!(s.share(), "share.home.smith.ca");
        s.check().expect("fine");
        let mut other = settings();
        other.domain = "jones.ca".into();
        assert!(other.check().is_err(), "not in the key's zone");
        let mut bad = settings();
        bad.domain = "smith".into();
        assert!(bad.check().is_err());
        let mut http = settings();
        http.acme_dns = Some(AcmeDns {
            url: "http://auth.example.org".into(),
            subdomain: "x".into(),
            username: "u".into(),
            password: "p".into(),
        });
        assert!(http.check().is_err(), "acme-dns over https only");
    }

    #[test]
    fn only_addresses_that_reach_the_box() {
        let mut r = crate::reach::Reach {
            ipv6: vec!["2a02:1810::7".parse().expect("ip")],
            port_mapped: Some("81.2.69.142:443".parse().expect("addr")),
            double_nat: Some(true),
            ..Default::default()
        };
        let (a, aaaa) = addresses(&r);
        assert!(
            a.is_empty(),
            "two routers: the A record would not reach the box"
        );
        assert_eq!(aaaa.len(), 1);
        r.double_nat = Some(false);
        assert_eq!(
            addresses(&r).0,
            vec![Value::A("81.2.69.142".parse().expect("ip"))]
        );
    }

    #[test]
    fn kept_private_and_forgotten() {
        let dir = tempfile::tempdir().expect("dir");
        let f = dir.path().join("names.json");
        let n = Names::open(&f).expect("open");
        assert!(n.settings().is_none());
        n.set(Some(settings())).expect("set");
        assert_eq!(Names::open(&f).expect("again").settings(), Some(settings()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&f).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        n.set(None).expect("forget");
        assert!(!f.exists());
    }

    #[test]
    fn bundles_days_and_serials() {
        let dir = tempfile::tempdir().expect("dir");
        let mut params =
            rcgen::CertificateParams::new(vec!["home.smith.ca".into()]).expect("params");
        params.not_after = rcgen::date_time_ymd(2027, 1, 31);
        params.serial_number = Some(rcgen::SerialNumber::from_slice(&[0x0a, 0xbc]));
        let key = rcgen::KeyPair::generate().expect("key");
        let cert = params.self_signed(&key).expect("cert");
        write_bundle(
            &dir.path().join("certificate"),
            &cert.pem(),
            &key.serialize_pem(),
        )
        .expect("write");
        let bundle = dir.path().join("certificate/connect.pem");
        assert_eq!(days_left(&bundle, 1_798_761_600), Some(30));
        assert_eq!(serial(&cert.pem()).as_deref(), Some("0abc"));
    }

    #[test]
    fn dates_as_the_logs_write_them() {
        assert_eq!(today(0), "1970-01-01");
        assert_eq!(today(1_798_761_600), "2027-01-01");
        assert_eq!(today(1_709_164_800), "2024-02-29");
    }
}
