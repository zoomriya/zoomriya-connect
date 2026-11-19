//! The box's certificate for its name and its guests' door, through the DNS
//! check: the control plane puts the temporary records, the key is made on
//! the box and never leaves it (docs/sharing-and-names.md, "Certificates").
//!
//! Written to `connect.pem` (the chain, then the key) in connectd's own
//! folder, made beside it first and moved in whole. On ZOOMRIYA OS the agent
//! checks it and serves it beside the box's own certificate
//! (zoomriya-os docs/design.md §8.1).

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use data_encoding::BASE64URL_NOPAD;
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, ExternalAccountKey,
    Identifier, NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use tracing::{info, warn};
use zoomriya_keys::SecretKey;
use zoomriya_proto::AcmeTarget;

use crate::{config::Authority, server::Server};

/// Renew when the certificate has fewer days left than this.
pub const RENEW_DAYS: i64 = 30;
/// The bundle's file name in `certs/`.
pub const BUNDLE: &str = "connect.pem";

/// Days left on the bundle's certificate, if there is one and it reads.
pub fn days_left(bundle: &Path, now: u64) -> Option<i64> {
    let pem = std::fs::read(bundle).ok()?;
    let (_, p) = x509_parser::pem::parse_x509_pem(&pem).ok()?;
    let cert = p.parse_x509().ok()?;
    let until = cert.validity().not_after.timestamp();
    Some((until - now as i64).div_euclid(86_400))
}

fn slug(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

async fn account(a: &Authority, data_dir: &Path) -> Result<Account> {
    let file = data_dir.join(format!("acme-{}.json", slug(&a.name)));
    if let Ok(bytes) = std::fs::read(&file) {
        let creds: AccountCredentials = serde_json::from_slice(&bytes)?;
        return Ok(Account::builder()?.from_credentials(creds).await?);
    }
    let eab = match (&a.eab_kid, &a.eab_hmac_key) {
        (Some(kid), Some(key)) => Some(ExternalAccountKey::new(
            kid.clone(),
            &BASE64URL_NOPAD
                .decode(key.trim_end_matches('=').as_bytes())
                .context("eab_hmac_key isn't base64url")?,
        )),
        _ => None,
    };
    let (account, creds) = Account::builder()?
        .create(
            &NewAccount {
                contact: &[],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            a.directory.clone(),
            eab.as_ref(),
        )
        .await
        .with_context(|| format!("making an account with {}", a.name))?;
    crate::files::write_private(&file, &serde_json::to_vec(&creds)?)?;
    Ok(account)
}

#[allow(clippy::too_many_arguments)]
/// Gets a certificate for `fqdn` and `share_fqdn` from the first authority
/// that gives one, and writes the bundle. Returns the authority's name.
pub async fn obtain(
    server: &Server,
    dns_key: &SecretKey,
    name: &str,
    fqdn: &str,
    share_fqdn: &str,
    authorities: &[Authority],
    data_dir: &Path,
    certs_dir: &Path,
) -> Result<String> {
    let mut last = anyhow!("no certificate authority is set");
    for a in authorities {
        match obtain_from(
            server, dns_key, name, fqdn, share_fqdn, a, data_dir, certs_dir,
        )
        .await
        {
            Ok(()) => {
                info!(authority = %a.name, "certificate for {fqdn}");
                return Ok(a.name.clone());
            }
            Err(e) => {
                warn!(authority = %a.name, "certificate: {e:#}");
                last = e;
            }
        }
        // Whatever happened, the check's records go.
        let _ = server
            .set_acme(dns_key, name, AcmeTarget::Box, vec![])
            .await;
        let _ = server
            .set_acme(dns_key, name, AcmeTarget::Door, vec![])
            .await;
    }
    Err(last)
}

#[allow(clippy::too_many_arguments)]
async fn obtain_from(
    server: &Server,
    dns_key: &SecretKey,
    name: &str,
    fqdn: &str,
    share_fqdn: &str,
    a: &Authority,
    data_dir: &Path,
    certs_dir: &Path,
) -> Result<()> {
    let account = account(a, data_dir).await?;
    let ids = [
        Identifier::Dns(fqdn.into()),
        Identifier::Dns(share_fqdn.into()),
    ];
    let mut order = account.new_order(&NewOrder::new(&ids)).await?;

    // What the authority wants to see, for each name.
    let (mut for_box, mut for_door) = (Vec::new(), Vec::new());
    {
        let mut auths = order.authorizations();
        while let Some(auth) = auths.next().await {
            let mut auth = auth?;
            if auth.status == AuthorizationStatus::Valid {
                continue;
            }
            let ch = auth
                .challenge(ChallengeType::Dns01)
                .context("the authority offers no DNS check")?;
            let id = ch.identifier().to_string();
            let value = ch.key_authorization().dns_value();
            if id == fqdn {
                for_box.push(value);
            } else if id == share_fqdn {
                for_door.push(value);
            } else {
                bail!("the authority asked about {id}");
            }
        }
    }
    if !for_box.is_empty() {
        server
            .set_acme(dns_key, name, AcmeTarget::Box, for_box)
            .await?;
    }
    if !for_door.is_empty() {
        server
            .set_acme(dns_key, name, AcmeTarget::Door, for_door)
            .await?;
    }
    // Our name servers answer from memory: a moment is enough.
    tokio::time::sleep(Duration::from_secs(5)).await;
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
    if status != OrderStatus::Ready {
        bail!("the authority's check ended {status:?}");
    }
    let key_pem = order.finalize().await?;
    let chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;
    server
        .set_acme(dns_key, name, AcmeTarget::Box, vec![])
        .await?;
    server
        .set_acme(dns_key, name, AcmeTarget::Door, vec![])
        .await?;
    write_bundle(certs_dir, &chain_pem, &key_pem)
}

/// Writes `connect.pem` beside `certs/` and moves it in: readable by the
/// group that serves HTTPS, as the box's own certificate is.
pub fn write_bundle(certs_dir: &Path, chain_pem: &str, key_pem: &str) -> Result<()> {
    let parent = certs_dir.parent().unwrap_or(certs_dir);
    std::fs::create_dir_all(certs_dir)
        .with_context(|| format!("making {}", certs_dir.display()))?;
    let tmp = parent.join(format!(".{BUNDLE}.new"));
    let mut bytes = chain_pem.trim_end().to_string();
    bytes.push('\n');
    bytes.push_str(key_pem.trim_end());
    bytes.push('\n');
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o640);
        }
        let mut f = opts
            .open(&tmp)
            .with_context(|| format!("writing {}", tmp.display()))?;
        f.write_all(bytes.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, certs_dir.join(BUNDLE))
        .with_context(|| format!("moving the certificate into {}", certs_dir.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_and_days_left() {
        let dir = tempfile::tempdir().expect("dir");
        let certs = dir.path().join("certs");
        assert_eq!(days_left(&certs.join(BUNDLE), 0), None);
        let mut params =
            rcgen::CertificateParams::new(vec!["maplehouse.zoomriya.app".into()]).expect("params");
        params.not_after = rcgen::date_time_ymd(2027, 1, 31);
        let key = rcgen::KeyPair::generate().expect("key");
        let cert = params.self_signed(&key).expect("cert");
        write_bundle(&certs, &cert.pem(), &key.serialize_pem()).expect("write");
        let jan_1_2027 = 1_798_761_600;
        assert_eq!(days_left(&certs.join(BUNDLE), jan_1_2027), Some(30));
        let text = std::fs::read_to_string(certs.join(BUNDLE)).expect("read");
        assert!(text.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(text.contains("PRIVATE KEY-----"));
        assert!(!dir.path().join(".connect.pem.new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(certs.join(BUNDLE))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o640);
        }
    }
}
