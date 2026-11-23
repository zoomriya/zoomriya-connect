//! A pretend phone, for trying Direct+ by hand: it pairs from a QR code's
//! URL at home, then reaches the box at the direct addresses the box gave it,
//! with no ZOOMRIYA server and no relay.
//!
//! ```text
//! cargo run --example test-phone -- pair  'zoomriya://pair?...' phone.json
//! cargo run --example test-phone -- link  phone.json
//! ```

use std::{net::SocketAddr, time::Duration};

use anyhow::{Context, Result, bail};
use data_encoding::BASE64URL_NOPAD;
use iroh::{Endpoint, EndpointAddr, RelayMode, TransportAddr, endpoint::presets};
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
    box_key: String,
    /// Where the box can be reached: home addresses and what it said.
    addrs: Vec<SocketAddr>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [c, url, file] if c == "pair" => pair(url, file).await,
        [c, file] if c == "link" => link(file).await,
        _ => bail!("usage: test-phone pair URL FILE | link FILE"),
    }
}

async fn endpoint(key: SecretKey) -> Result<Endpoint> {
    Ok(Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?)
}

async fn pair(url: &str, file: &str) -> Result<()> {
    let code = PairingCode::from_url(url).context("not a pairing code")?;
    let chip = zoomriya_keys::new_test_chip();
    let link = SecretKey::generate();
    let ep = endpoint(link.clone()).await?;
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
    };
    let mut body = serde_json::to_vec(&req)?;
    body.push(b'\n');
    send.write_all(&body).await?;
    send.finish()?;
    let mut line = String::new();
    BufReader::new(recv).read_line(&mut line).await?;
    match serde_json::from_str::<PairResponse>(&line)? {
        PairResponse::Paired { addrs, .. } => {
            let mut all = code.addrs.clone();
            all.extend(addrs);
            all.sort();
            all.dedup();
            let saved = Saved {
                chip: BASE64URL_NOPAD.encode(&chip.to_bytes()),
                link: BASE64URL_NOPAD.encode(&link.to_bytes()),
                box_key: code.box_key.to_string(),
                addrs: all,
            };
            std::fs::write(file, serde_json::to_vec_pretty(&saved)?)?;
            println!("paired");
        }
        PairResponse::Refused { message, .. } => bail!("refused: {message}"),
        PairResponse::Challenge { .. } => bail!("the box answered as to a laptop"),
    }
    conn.close(0u32.into(), b"done");
    Ok(())
}

async fn link(file: &str) -> Result<()> {
    let saved: Saved = serde_json::from_slice(&std::fs::read(file)?)?;
    let bytes: [u8; 32] = BASE64URL_NOPAD
        .decode(saved.link.as_bytes())?
        .try_into()
        .map_err(|_| anyhow::anyhow!("key"))?;
    let ep = endpoint(SecretKey::from_bytes(&bytes)).await?;
    let box_key = zoomriya_keys::key_from_str(&saved.box_key)?;
    let addr = EndpointAddr::from_parts(box_key, saved.addrs.iter().map(|a| TransportAddr::Ip(*a)));
    let conn = tokio::time::timeout(Duration::from_secs(20), ep.connect(addr, LINK_ALPN))
        .await
        .context("timed out reaching the box")?
        .context("reaching the box directly")?;
    let (mut ctl, recv) = conn.open_bi().await?;
    ctl.write_all(&[STREAM_CONTROL]).await?;
    let mut hello = serde_json::to_vec(&ToBox::Hello { version: 1 })?;
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
    send.write_all(&[STREAM_WEB, 7]).await?;
    send.write_all(b"gateway").await?;
    send.write_all(b"GET / HTTP/1.0\r\nHost: box\r\n\r\n")
        .await?;
    send.finish()?;
    let mut reply = String::new();
    tokio::time::timeout(Duration::from_secs(10), recv.read_to_string(&mut reply))
        .await
        .context("no web answer")??;
    println!("web says: {}", reply.lines().next().unwrap_or(""));
    conn.close(0u32.into(), b"done");
    Ok(())
}
