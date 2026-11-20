//! Just enough STUN (RFC 8489): one binding request and its answer. connectd
//! learns the box's public address on the laptops' WebRTC port with it, and a
//! relay checks the TURN server beside it is answering.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const COOKIE: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// A binding request with this transaction ID.
pub fn request(txid: &[u8; 12]) -> [u8; 20] {
    let mut out = [0u8; 20];
    out[..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Length 0: no attributes.
    out[4..8].copy_from_slice(&COOKIE.to_be_bytes());
    out[8..].copy_from_slice(txid);
    out
}

/// The address a binding success answer for `txid` says it saw, if `buf` is one.
pub fn mapped(buf: &[u8], txid: &[u8; 12]) -> Option<SocketAddr> {
    if buf.len() < 20
        || u16::from_be_bytes([buf[0], buf[1]]) != BINDING_SUCCESS
        || buf[4..8] != COOKIE.to_be_bytes()
        || buf[8..20] != txid[..]
    {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
    let attrs = buf.get(20..20 + len)?;
    let mut plain = None;
    let mut at = 0;
    while at + 4 <= attrs.len() {
        let kind = u16::from_be_bytes([attrs[at], attrs[at + 1]]);
        let n = usize::from(u16::from_be_bytes([attrs[at + 2], attrs[at + 3]]));
        let value = attrs.get(at + 4..at + 4 + n)?;
        match kind {
            XOR_MAPPED_ADDRESS => return address(value, Some(txid)),
            MAPPED_ADDRESS => plain = address(value, None),
            _ => {}
        }
        at += 4 + n.div_ceil(4) * 4;
    }
    plain
}

fn address(v: &[u8], xor: Option<&[u8; 12]>) -> Option<SocketAddr> {
    let family = *v.get(1)?;
    let mut port = u16::from_be_bytes([*v.get(2)?, *v.get(3)?]);
    let cookie = COOKIE.to_be_bytes();
    if xor.is_some() {
        port ^= (COOKIE >> 16) as u16;
    }
    let ip = match family {
        1 => {
            let mut b: [u8; 4] = v.get(4..8)?.try_into().ok()?;
            if xor.is_some() {
                for (x, c) in b.iter_mut().zip(cookie) {
                    *x ^= c;
                }
            }
            IpAddr::V4(Ipv4Addr::from(b))
        }
        2 => {
            let mut b: [u8; 16] = v.get(4..20)?.try_into().ok()?;
            if let Some(txid) = xor {
                let key: Vec<u8> = cookie.iter().chain(txid.iter()).copied().collect();
                for (x, k) in b.iter_mut().zip(key) {
                    *x ^= k;
                }
            }
            IpAddr::V6(Ipv6Addr::from(b))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// `stun:host:port` to `host:port` (3478 when no port is given).
pub fn host_port(url: &str) -> Option<String> {
    let rest = url.strip_prefix("stun:")?;
    let rest = rest.split('?').next().unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    let has_port = if rest.starts_with('[') {
        rest.contains("]:")
    } else {
        rest.rsplit_once(':')
            .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    };
    Some(if has_port {
        rest.to_string()
    } else {
        format!("{rest}:3478")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_answer() {
        let txid = [7u8; 12];
        // A binding success with XOR-MAPPED-ADDRESS 203.0.113.9:40000.
        let ip = [203u8, 0, 113, 9];
        let cookie = COOKIE.to_be_bytes();
        let mut attr = vec![0x00, 0x20, 0x00, 0x08, 0x00, 0x01];
        attr.extend_from_slice(&(40000u16 ^ (COOKIE >> 16) as u16).to_be_bytes());
        attr.extend(ip.iter().zip(cookie).map(|(a, c)| a ^ c));
        let mut msg = vec![0x01, 0x01];
        msg.extend_from_slice(&(attr.len() as u16).to_be_bytes());
        msg.extend_from_slice(&cookie);
        msg.extend_from_slice(&txid);
        msg.extend_from_slice(&attr);
        assert_eq!(
            mapped(&msg, &txid),
            Some("203.0.113.9:40000".parse().unwrap())
        );
        // Someone else's transaction.
        assert_eq!(mapped(&msg, &[8u8; 12]), None);
        assert_eq!(mapped(&request(&txid), &txid), None);
    }

    #[test]
    fn urls() {
        assert_eq!(
            host_port("stun:turn.zoomriya.net:3478").as_deref(),
            Some("turn.zoomriya.net:3478")
        );
        assert_eq!(
            host_port("stun:turn.zoomriya.net").as_deref(),
            Some("turn.zoomriya.net:3478")
        );
        assert_eq!(host_port("turn:x:1"), None);
    }
}
