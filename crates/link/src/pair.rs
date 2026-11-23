//! Pairing a laptop with the short code Box Desktop shows, at home
//! ([`zoomriya_proto::laptop`]). The code is never sent: SPAKE2 proves both
//! sides know it, so the box key the laptop pins is the real box's.

use std::{net::SocketAddr, time::Duration};

use data_encoding::BASE64URL_NOPAD;
use iroh::{Endpoint, EndpointAddr, TransportAddr};
use tokio::io::{AsyncBufReadExt, BufReader};
use zoomriya_keys::{
    PublicKey,
    pake::{self, Side},
};
use zoomriya_proto::{
    laptop::{LaptopFinish, LaptopHello, normalise_code},
    link::{MAX_LINE, PAIR_ALPN, PairResponse},
};

use crate::device::Signer;

/// Why pairing did not work, in words for the person pairing.
#[derive(Debug, thiserror::Error)]
pub enum PairError {
    /// The box could not be reached.
    #[error("The box could not be reached. Check that this laptop is on the home network.")]
    Unreachable,
    /// The box only pairs over the home network.
    #[error("Pair at home: this laptop must be on the same network as the box.")]
    NotHome,
    /// The code did not match, or the box is not the one showing it.
    #[error("That code does not match. Check it on the box and type it again.")]
    WrongCode,
    /// The box said no.
    #[error("{0}")]
    Refused(String),
    /// Something else went wrong on the way.
    #[error("Pairing did not work. Try again.")]
    Broken,
}

/// What pairing gave the laptop.
#[derive(Debug, Clone)]
pub struct Paired {
    /// The box key, now pinned.
    pub box_key: PublicKey,
    /// Where the box can be reached directly.
    pub addrs: Vec<SocketAddr>,
}

const STEP: Duration = Duration::from_secs(20);

async fn line<R: tokio::io::AsyncRead + Unpin>(r: &mut BufReader<R>) -> Result<String, PairError> {
    let mut s = String::new();
    let n = tokio::time::timeout(STEP, r.read_line(&mut s))
        .await
        .map_err(|_| PairError::Broken)?
        .map_err(|_| PairError::Broken)?;
    if n == 0 || s.len() > MAX_LINE {
        return Err(PairError::Broken);
    }
    Ok(s)
}

fn json_line<T: serde::Serialize>(v: &T) -> Vec<u8> {
    let mut b = serde_json::to_vec(v).unwrap_or_default();
    b.push(b'\n');
    b
}

/// Pairs with the box at `box_key`, reached at `addrs`, with the code as
/// typed. `ep` must use the laptop's link key.
pub async fn laptop(
    ep: &Endpoint,
    box_key: PublicKey,
    addrs: &[SocketAddr],
    code: &str,
    device: &dyn Signer,
    name: &str,
) -> Result<Paired, PairError> {
    // Home addresses only: the box pairs only over the home network, and
    // would refuse a connection over the home's public IPv6. (Loopback is
    // for tests; a real box refuses it.)
    let home: Vec<SocketAddr> = addrs
        .iter()
        .copied()
        .filter(|a| zoomriya_proto::addrs::is_home(a.ip()) || a.ip().is_loopback())
        .collect();
    if home.is_empty() {
        return Err(PairError::NotHome);
    }
    let to = EndpointAddr::from_parts(box_key, home.iter().map(|a| TransportAddr::Ip(*a)));
    let conn = tokio::time::timeout(STEP, ep.connect(to, PAIR_ALPN))
        .await
        .map_err(|_| PairError::Unreachable)?
        .map_err(|_| PairError::Unreachable)?;
    let result = async {
        let (mut send, recv) = conn.open_bi().await.map_err(|_| PairError::Broken)?;
        let mut recv = BufReader::new(recv);
        let start = pake::start(&normalise_code(code));
        let laptop_msg = start.msg.clone();
        send.write_all(&json_line(&LaptopHello {
            spake: BASE64URL_NOPAD.encode(&laptop_msg),
        }))
        .await
        .map_err(|_| PairError::Broken)?;

        let answer: PairResponse =
            serde_json::from_str(&line(&mut recv).await?).map_err(|_| PairError::Broken)?;
        let (box_msg, confirm) = match answer {
            PairResponse::Challenge { spake, confirm } => (spake, confirm),
            PairResponse::Refused { code, message } => {
                return Err(if code == "pairing_code" {
                    PairError::WrongCode
                } else {
                    PairError::Refused(message)
                });
            }
            PairResponse::Paired { .. } => return Err(PairError::Broken),
        };
        let box_msg = BASE64URL_NOPAD
            .decode(box_msg.as_bytes())
            .map_err(|_| PairError::Broken)?;
        let confirm = BASE64URL_NOPAD
            .decode(confirm.as_bytes())
            .map_err(|_| PairError::Broken)?;
        let link = ep.id();
        let t = pake::transcript(&box_key, &link, &laptop_msg, &box_msg);
        let session = start.finish(&box_msg).map_err(|_| PairError::WrongCode)?;
        // The box must prove it knows the code before anything is pinned.
        session
            .check(Side::Box, &t, b"", &confirm)
            .map_err(|_| PairError::WrongCode)?;

        let mut finish = LaptopFinish {
            device_key: device.public_b64(),
            link_sig: device.sign_link(&link),
            name: name.chars().take(60).collect(),
            confirm: String::new(),
        };
        finish.confirm =
            BASE64URL_NOPAD.encode(&session.confirm(Side::Laptop, &t, &finish.extra()));
        send.write_all(&json_line(&finish))
            .await
            .map_err(|_| PairError::Broken)?;
        send.finish().map_err(|_| PairError::Broken)?;
        match serde_json::from_str(&line(&mut recv).await?).map_err(|_| PairError::Broken)? {
            PairResponse::Paired { addrs: said } => {
                let mut all = addrs.to_vec();
                all.extend(said);
                all.sort();
                all.dedup();
                Ok(Paired {
                    box_key,
                    addrs: all,
                })
            }
            PairResponse::Refused { code, message } => Err(if code == "pairing_code" {
                PairError::WrongCode
            } else {
                PairError::Refused(message)
            }),
            PairResponse::Challenge { .. } => Err(PairError::Broken),
        }
    }
    .await;
    // The box closes at once when the connection is not from home.
    let not_home = conn
        .close_reason()
        .is_some_and(|r| format!("{r:?}").contains("pair at home"));
    conn.close(0u32.into(), b"done");
    match result {
        Err(_) if not_home => Err(PairError::NotHome),
        r => r,
    }
}
