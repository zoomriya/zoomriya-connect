//! A pretend phone, for trying a whole ZOOMRIYA Connect setup by hand: it
//! pairs from a QR code's URL, then reaches the box through the relay only,
//! as a phone away from home would.
//!
//! ```text
//! cargo run --example test-phone -- pair  'zoomriya://pair?...' phone.json
//! cargo run --example test-phone -- link  phone.json RELAY_URL [CA_PEM]
//! ```

use std::time::Duration;

use anyhow::{Context, Result, bail};
use data_encoding::BASE64URL_NOPAD;
use iroh::{
    Endpoint, EndpointAddr, RelayMap, RelayMode, RelayUrl, TransportAddr, endpoint::presets,
};
use p256::ecdsa::{Signature, signature::Signer};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use zoomriya_keys::{SecretKey, link_key_message};
use zoomriya_proto::link::{
    FromBox, LINK_ALPN, PAIR_ALPN, PairRequest, PairResponse, PairingCode, STREAM_CONTROL,
    STREAM_WEB, ToBox,
};

#[derive(Serialize, Deserialize)]
struct Saved {
    chip: String,
    link: String,
    pass: String,
    box_key: String,
    fqdn: String,
}

fn roots(ca: Option<&String>) -> Result<iroh::tls::CaTlsConfig> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    let extra = match ca {
        Some(f) => CertificateDer::pem_file_iter(f)?.collect::<Result<Vec<_>, _>>()?,
        None => vec![],
    };
    Ok(iroh::tls::CaTlsConfig::default().with_extra_roots(extra))
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("pair") => pair(&args[1], &args[2]).await,
        Some("link") => link(&args[1], &args[2], args.get(3)).await,
        _ => bail!("usage: test-phone pair URL FILE | link FILE RELAY_URL [CA_PEM]"),
    }
}

async fn pair(url: &str, file: &str) -> Result<()> {
    let code = PairingCode::from_url(url).context("not a pairing code")?;
    let chip = zoomriya_keys::new_test_chip();
    let link = SecretKey::generate();
    let ep = Endpoint::builder(presets::Minimal)
        .secret_key(link.clone())
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;
    let addr = EndpointAddr::from_parts(
        code.box_key.0,
        code.addrs.iter().map(|a| TransportAddr::Ip(*a)),
    );
    let conn = ep
        .connect(addr, PAIR_ALPN)
        .await
        .context("reaching the box at home")?;
    let (mut send, recv) = conn.open_bi().await?;
    let sig: Signature = chip.sign(&link_key_message(&link.public()));
    let req = PairRequest {
        secret: code.secret,
        device_key: BASE64URL_NOPAD.encode(chip.verifying_key().to_sec1_point(true).as_bytes()),
        link: link.public().into(),
        link_sig: BASE64URL_NOPAD.encode(sig.to_der().as_bytes()),
        name: "Test phone".into(),
        admin_key: None,
    };
    let mut body = serde_json::to_vec(&req)?;
    body.push(b'\n');
    send.write_all(&body).await?;
    send.finish()?;
    let mut line = String::new();
    BufReader::new(recv).read_line(&mut line).await?;
    match serde_json::from_str::<PairResponse>(&line)? {
        PairResponse::Paired { pass, fqdn, .. } => {
            let saved = Saved {
                chip: BASE64URL_NOPAD.encode(&chip.to_bytes()),
                link: BASE64URL_NOPAD.encode(&link.to_bytes()),
                pass,
                box_key: code.box_key.to_string(),
                fqdn: fqdn.clone(),
            };
            std::fs::write(file, serde_json::to_vec_pretty(&saved)?)?;
            println!("paired with {fqdn}");
        }
        PairResponse::Refused { message, .. } => bail!("refused: {message}"),
    }
    conn.close(0u32.into(), b"done");
    Ok(())
}

async fn link(file: &str, relay: &str, ca: Option<&String>) -> Result<()> {
    let saved: Saved = serde_json::from_slice(&std::fs::read(file)?)?;
    let bytes: [u8; 32] = BASE64URL_NOPAD
        .decode(saved.link.as_bytes())?
        .try_into()
        .map_err(|_| anyhow::anyhow!("key"))?;
    let link = SecretKey::from_bytes(&bytes);
    let relay: RelayUrl = relay.parse()?;
    // Away from home: our relay only, with the pass as the relay's token.
    let map = RelayMap::from(relay.clone()).with_auth_token(saved.pass.clone());
    let ep = Endpoint::builder(presets::Minimal)
        .secret_key(link)
        .relay_mode(RelayMode::Custom(map))
        .ca_tls_config(roots(ca)?)
        .clear_ip_transports()
        .bind()
        .await?;
    let box_key = zoomriya_keys::key_from_str(&saved.box_key)?;
    let addr = EndpointAddr::new(box_key).with_relay_url(relay);
    let conn = tokio::time::timeout(Duration::from_secs(20), ep.connect(addr, LINK_ALPN))
        .await
        .context("timed out reaching the box")?
        .context("reaching the box through the relay")?;
    let (mut ctl, recv) = conn.open_bi().await?;
    ctl.write_all(&[STREAM_CONTROL]).await?;
    let mut hello = serde_json::to_vec(&ToBox::Hello {
        version: 1,
        pass_until: 0,
    })?;
    hello.push(b'\n');
    ctl.write_all(&hello).await?;
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        BufReader::new(recv).read_line(&mut line),
    )
    .await
    .context("no welcome")??;
    let welcome: FromBox = serde_json::from_str(&line)?;
    println!("box says: {welcome:?}");
    let (mut send, mut recv) = conn.open_bi().await?;
    send.write_all(&[STREAM_WEB, 3]).await?;
    send.write_all(b"web").await?;
    send.write_all(format!("GET / HTTP/1.0\r\nHost: {}\r\n\r\n", saved.fqdn).as_bytes())
        .await?;
    send.finish()?;
    let mut reply = String::new();
    tokio::time::timeout(Duration::from_secs(10), recv.read_to_string(&mut reply))
        .await
        .context("no web answer")??;
    println!("web says: {}", reply.lines().next().unwrap_or(""));
    let relayed = conn.paths().iter().all(|p| p.is_relay());
    println!("through the relay only: {relayed}");
    conn.close(0u32.into(), b"done");
    Ok(())
}
