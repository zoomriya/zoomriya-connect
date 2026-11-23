//! The family's own DNS records, changed by the box through the standard
//! way (RFC 2136 dynamic update, signed with a TSIG key; docs/direct-plus.md
//! §11). No provider's own API.
//!
//! Each change replaces a whole record set in one signed message: the old
//! records go and the new ones come, or nothing changes.

use std::{net::SocketAddr, time::Duration};

use anyhow::{Context, Result, bail};
use data_encoding::BASE64;
use hickory_proto::{
    op::{Message, OpCode, Query, ResponseCode, UpdateMessage},
    rr::{
        DNSClass, Name, RData, Record, RecordType, TSigner,
        rdata::{A, AAAA, TXT, tsig::TsigAlgorithm},
    },
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

/// How long the server gets to answer.
const ANSWER: Duration = Duration::from_secs(15);
/// How far apart the box's clock and the server's may be.
const FUDGE: u16 = 300;

/// Where and how to change the family's records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tsig {
    /// The zone's primary server, which takes updates: a name or an
    /// address, with `:port` if not 53.
    pub server: String,
    /// The zone, such as `smith.ca`.
    pub zone: String,
    /// The key's name, as the server knows it.
    pub key_name: String,
    /// `hmac-sha256` (the usual), `hmac-sha384` or `hmac-sha512`.
    #[serde(default = "sha256")]
    pub algorithm: String,
    /// The key, base64.
    pub secret: String,
}

fn sha256() -> String {
    "hmac-sha256".into()
}

/// What a record holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// An IPv4 address.
    A(std::net::Ipv4Addr),
    /// An IPv6 address.
    Aaaa(std::net::Ipv6Addr),
    /// Text: the certificate authority's check.
    Txt(String),
}

impl Value {
    fn rdata(&self) -> RData {
        match self {
            Self::A(a) => RData::A(A(*a)),
            Self::Aaaa(a) => RData::AAAA(AAAA(*a)),
            Self::Txt(t) => RData::TXT(TXT::new(vec![t.clone()])),
        }
    }
}

fn fqdn(s: &str) -> Result<Name> {
    let mut n =
        Name::from_ascii(s.trim_end_matches('.')).with_context(|| format!("{s}: not a name"))?;
    n.set_fqdn(true);
    Ok(n)
}

impl Tsig {
    fn signer(&self) -> Result<TSigner> {
        let algorithm = match self.algorithm.to_ascii_lowercase().as_str() {
            "hmac-sha256" => TsigAlgorithm::HmacSha256,
            "hmac-sha384" => TsigAlgorithm::HmacSha384,
            "hmac-sha512" => TsigAlgorithm::HmacSha512,
            other => bail!("the key's algorithm {other} is not supported: use hmac-sha256"),
        };
        let key = BASE64
            .decode(self.secret.trim().as_bytes())
            .context("the key is not base64")?;
        TSigner::new(key, algorithm, fqdn(&self.key_name)?, FUDGE)
            .map_err(|e| anyhow::anyhow!("the key: {e}"))
    }

    async fn address(&self) -> Result<SocketAddr> {
        let s = self.server.trim();
        let with_port = if s.parse::<SocketAddr>().is_ok()
            || s.rsplit_once(':')
                .is_some_and(|(h, p)| !h.contains(':') && p.parse::<u16>().is_ok())
        {
            s.to_string()
        } else if s.contains(':') && !s.starts_with('[') {
            format!("[{s}]:53") // a bare IPv6 address
        } else {
            format!("{s}:53")
        };
        tokio::net::lookup_host(&with_port)
            .await
            .with_context(|| format!("finding the DNS server {s}"))?
            .next()
            .with_context(|| format!("finding the DNS server {s}"))
    }

    /// Checks the settings without sending anything.
    pub fn check(&self) -> Result<()> {
        anyhow::ensure!(!self.server.trim().is_empty(), "give the DNS server");
        self.signer()?;
        fqdn(&self.zone)?;
        Ok(())
    }

    /// The signed message replacing `name`'s records of `kind` with
    /// `values` (none: they go).
    fn message(
        &self,
        name: &str,
        kind: RecordType,
        values: &[Value],
        ttl: u32,
        now: u64,
    ) -> Result<Message> {
        let zone = fqdn(&self.zone)?;
        let name = fqdn(name)?;
        anyhow::ensure!(zone.zone_of(&name), "{name} is not in {zone}");
        let mut query = Query::new();
        query
            .set_name(zone)
            .set_query_class(DNSClass::IN)
            .set_query_type(RecordType::SOA);
        let mut m = Message::query();
        m.metadata.op_code = OpCode::Update;
        m.metadata.recursion_desired = false;
        m.add_zone(query);
        // The whole set goes...
        let mut gone = Record::update0(name.clone(), 0, kind);
        gone.dns_class = DNSClass::ANY;
        m.add_update(gone.into_record_of_rdata());
        // ...and the new one comes, in the same message.
        for v in values {
            m.add_update(Record::from_rdata(name.clone(), ttl, v.rdata()));
        }
        m.finalize(&self.signer()?, now)
            .map_err(|e| anyhow::anyhow!("signing: {e}"))?;
        Ok(m)
    }

    /// Replaces `name`'s records of one kind with `values`, all of that
    /// kind (an empty list removes them).
    pub async fn replace(
        &self,
        name: &str,
        kind: RecordType,
        values: &[Value],
        ttl: u32,
    ) -> Result<()> {
        let m = self.message(name, kind, values, ttl, crate::now())?;
        let bytes = m.to_vec().context("encoding")?;
        let answer = tokio::time::timeout(ANSWER, async {
            let mut s = TcpStream::connect(self.address().await?).await?;
            s.write_all(&u16::try_from(bytes.len())?.to_be_bytes())
                .await?;
            s.write_all(&bytes).await?;
            let len = s.read_u16().await? as usize;
            let mut buf = vec![0; len];
            s.read_exact(&mut buf).await?;
            anyhow::Ok(buf)
        })
        .await
        .context("the DNS server did not answer")??;
        let answer = Message::from_vec(&answer).context("the DNS server's answer")?;
        match answer.metadata.response_code {
            ResponseCode::NoError => Ok(()),
            ResponseCode::NotAuth => bail!("the DNS server refused the key"),
            ResponseCode::Refused => bail!("the DNS server refused the change"),
            ResponseCode::NotZone => bail!("{name} is not in the zone {}", self.zone),
            other => bail!("the DNS server answered {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tsig() -> Tsig {
        Tsig {
            server: "127.0.0.1:53".into(),
            zone: "smith.ca".into(),
            key_name: "box-key".into(),
            algorithm: sha256(),
            secret: BASE64.encode(&[7u8; 32]),
        }
    }

    #[test]
    fn a_signed_replacement() {
        let t = tsig();
        t.check().expect("valid");
        let m = t
            .message(
                "home.smith.ca",
                RecordType::AAAA,
                &[Value::Aaaa("2a02:1810::7".parse().expect("ip"))],
                60,
                1_795_000_000,
            )
            .expect("message");
        assert_eq!(m.metadata.op_code, OpCode::Update);
        let bytes = m.to_vec().expect("bytes");
        // The server's side: the signature checks with the same key.
        t.signer()
            .expect("signer")
            .verify_message_byte(&bytes, None, true)
            .expect("signed with the key");
        let back = Message::from_vec(&bytes).expect("decodes");
        assert_eq!(back.updates().len(), 2, "the old set goes, the new comes");
    }

    /// A DNS server's side, by hand: reads one update, checks its
    /// signature, answers with `code`.
    async fn server(code: ResponseCode) -> (SocketAddr, tokio::task::JoinHandle<Message>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = l.local_addr().expect("addr");
        let task = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.expect("accept");
            let len = s.read_u16().await.expect("len") as usize;
            let mut buf = vec![0; len];
            s.read_exact(&mut buf).await.expect("read");
            tsig()
                .signer()
                .expect("signer")
                .verify_message_byte(&buf, None, true)
                .expect("signed");
            let asked = Message::from_vec(&buf).expect("decodes");
            let mut answer = Message::response(asked.metadata.id, OpCode::Update);
            answer.metadata.response_code = code;
            let out = answer.to_vec().expect("bytes");
            s.write_all(&(out.len() as u16).to_be_bytes())
                .await
                .expect("write");
            s.write_all(&out).await.expect("write");
            asked
        });
        (addr, task)
    }

    #[tokio::test]
    async fn sent_and_answered() {
        let (addr, task) = server(ResponseCode::NoError).await;
        let mut t = tsig();
        t.server = addr.to_string();
        t.replace(
            "_acme-challenge.home.smith.ca",
            RecordType::TXT,
            &[Value::Txt("abc".into())],
            60,
        )
        .await
        .expect("accepted");
        let asked = task.await.expect("server");
        assert_eq!(asked.updates().len(), 2);
        let (addr, _) = server(ResponseCode::NotAuth).await;
        t.server = addr.to_string();
        let e = t
            .replace("home.smith.ca", RecordType::A, &[], 60)
            .await
            .expect_err("refused");
        assert!(e.to_string().contains("refused the key"), "{e}");
    }

    #[test]
    fn names_outside_the_zone_and_bad_keys() {
        let t = tsig();
        assert!(
            t.message("home.jones.ca", RecordType::A, &[], 60, 0)
                .is_err()
        );
        let mut bad = tsig();
        bad.secret = "not base64!".into();
        assert!(bad.check().is_err());
        let mut md5 = tsig();
        md5.algorithm = "hmac-md5".into();
        assert!(md5.check().is_err());
    }
}
