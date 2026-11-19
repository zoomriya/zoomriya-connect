//! Authoritative DNS for box names and address records.
//!
//! | Name | Answer |
//! |---|---|
//! | `zoomriya.app` | SOA, NS, CAA (only our certificate authorities) |
//! | `maplehouse.zoomriya.app` | The box on the home network (A, AAAA) |
//! | `maplehouse-share.zoomriya.app` | The guests' door, while it's open |
//! | `_acme-challenge.maplehouse[-share].zoomriya.app` | Certificate-check TXT, for an hour |
//! | `_iroh.<box key>.dns.zoomriya.net` | The box's signed address record, as TXT |
//! | Anything else | Refused: we never look up other names (no recursion) |
//!
//! Over UDP, each /24 or /56 gets so many answers a second; beyond that the
//! answer is empty and marked truncated, so a real resolver comes back over
//! TCP and a flood gets nothing bigger than it sent.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use hickory_server::{
    net::{runtime::Time, xfer::Protocol},
    proto::{
        op::{Edns, Header, Metadata, OpCode, ResponseCode},
        rr::{
            Name, RData, Record, RecordType,
            rdata::{self, CAA, SOA, TXT},
        },
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    zone_handler::MessageResponseBuilder,
};
use iroh_dns::pkarr::SignedPacket;
use tokio::net::{TcpListener, UdpSocket};
use tracing::{debug, info};
use zoomriya_keys::PublicKey;
use zoomriya_proto::names::SHARE_SUFFIX;

use crate::{limit::Limiter, store::Store};

const TTL_HOME: u32 = 300;
const TTL_SHORT: u32 = 60;
const TTL_RECORD: u32 = 30;
const TTL_ZONE: u32 = 3600;
/// Also the time a "no such name" is remembered.
const NEGATIVE_TTL: u32 = 60;

/// The zones we answer for.
#[derive(Debug, Clone)]
pub struct Zones {
    /// Box names, such as `zoomriya.app`.
    pub boxes: String,
    /// Address records, such as `dns.zoomriya.net`.
    pub records: String,
    /// Name servers; the first goes in the SOA.
    pub nameservers: Vec<String>,
    /// The SOA's contact, as a name.
    pub hostmaster: String,
    /// Certificate authorities for CAA.
    pub certificate_authorities: Vec<String>,
}

/// What a lookup found.
#[derive(Debug, PartialEq)]
pub enum Found {
    /// These records.
    Records(Vec<Record>),
    /// The name exists, but not with that type.
    NoData,
    /// No such name.
    NxDomain,
    /// Not our zone.
    Refused,
}

/// Answers DNS from the store.
pub struct DnsHandler {
    store: Arc<Store>,
    zones: Zones,
    limiter: Limiter<IpAddr>,
    now: fn() -> u64,
}

fn name(s: &str) -> Result<Name> {
    Name::from_ascii(format!("{}.", s.trim_end_matches('.'))).with_context(|| format!("name {s}"))
}

fn txt(name: &Name, ttl: u32, value: &str) -> Record {
    // A TXT string holds at most 255 bytes; longer values are split.
    let parts = value
        .as_bytes()
        .chunks(255)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    Record::from_rdata(name.clone(), ttl, RData::TXT(TXT::new(parts)))
}

/// The network a DNS rate limit counts: the /24 or the /56.
fn subnet(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], 0))
        }
        IpAddr::V6(a) => match a.to_ipv4_mapped() {
            Some(v4) => subnet(IpAddr::V4(v4)),
            None => {
                let mut o = a.octets();
                o[7..].fill(0);
                IpAddr::V6(Ipv6Addr::from(o))
            }
        },
    }
}

impl DnsHandler {
    /// A handler answering `answers_per_second` per /24 or /56 over UDP.
    pub fn new(store: Arc<Store>, zones: Zones, answers_per_second: u32) -> Self {
        let rate = answers_per_second.max(1);
        Self {
            store,
            zones,
            limiter: Limiter::new(f64::from(rate), rate.saturating_mul(2)),
            now: zoomriya_serve::now,
        }
    }

    fn soa(&self, zone: &str) -> Option<Record> {
        let zone_name = name(zone).ok()?;
        let mname = name(self.zones.nameservers.first()?).ok()?;
        let rname = name(&self.zones.hostmaster).ok()?;
        let soa = SOA::new(
            mname,
            rname,
            self.store.serial(),
            3600,
            600,
            1_209_600,
            NEGATIVE_TTL,
        );
        Some(Record::from_rdata(zone_name, TTL_ZONE, RData::SOA(soa)))
    }

    fn apex(&self, zone: &str, qtype: RecordType) -> Found {
        let Ok(zone_name) = name(zone) else {
            return Found::Refused;
        };
        let records: Vec<Record> = match qtype {
            RecordType::SOA => self.soa(zone).into_iter().collect(),
            RecordType::NS => self
                .zones
                .nameservers
                .iter()
                .filter_map(|ns| name(ns).ok())
                .map(|ns| Record::from_rdata(zone_name.clone(), TTL_ZONE, RData::NS(rdata::NS(ns))))
                .collect(),
            RecordType::CAA if zone == self.zones.boxes => self
                .zones
                .certificate_authorities
                .iter()
                // The issuer is written without a final dot: CAs compare it as text.
                .filter_map(|ca| Name::from_ascii(ca.trim_end_matches('.')).ok())
                .map(|ca| {
                    Record::from_rdata(
                        zone_name.clone(),
                        TTL_ZONE,
                        RData::CAA(CAA::new_issue(false, Some(ca), vec![])),
                    )
                })
                .collect(),
            _ => vec![],
        };
        if records.is_empty() {
            Found::NoData
        } else {
            Found::Records(records)
        }
    }

    /// Looks a name up. `qname` is lower case, with or without the final dot.
    pub fn lookup(&self, qname: &str, qtype: RecordType) -> Found {
        let qname = qname.trim_end_matches('.').to_ascii_lowercase();
        let Ok(owner) = name(&qname) else {
            return Found::Refused;
        };
        for zone in [&self.zones.boxes, &self.zones.records] {
            if qname == *zone {
                return self.apex(zone, qtype);
            }
        }
        if let Some(rel) = qname.strip_suffix(&format!(".{}", self.zones.boxes)) {
            return self.box_name(rel, &owner, qtype);
        }
        if let Some(rel) = qname.strip_suffix(&format!(".{}", self.zones.records)) {
            return self.address_record(rel, &owner, qtype);
        }
        Found::Refused
    }

    fn box_name(&self, rel: &str, owner: &Name, qtype: RecordType) -> Found {
        let now = (self.now)();
        let labels: Vec<&str> = rel.split('.').collect();
        match labels.as_slice() {
            [label] => {
                let (base, door) = match label.strip_suffix(SHARE_SUFFIX) {
                    Some(base) => (base, true),
                    None => (*label, false),
                };
                let Some(e) = self.store.entry(base) else {
                    return Found::NxDomain;
                };
                let (v4, v6, ttl): (Vec<Ipv4Addr>, Vec<Ipv6Addr>, u32) = if door {
                    if e.door_until <= now {
                        // The door's name exists only while a link is live,
                        // but its certificate check may be under it.
                        return Found::NoData;
                    }
                    (
                        e.door_v4.into_iter().collect(),
                        e.door_v6.into_iter().collect(),
                        TTL_SHORT,
                    )
                } else {
                    (e.home.ipv4, e.home.ipv6, TTL_HOME)
                };
                let records: Vec<Record> = match qtype {
                    RecordType::A => v4
                        .into_iter()
                        .map(|a| Record::from_rdata(owner.clone(), ttl, RData::A(a.into())))
                        .collect(),
                    RecordType::AAAA => v6
                        .into_iter()
                        .map(|a| Record::from_rdata(owner.clone(), ttl, RData::AAAA(a.into())))
                        .collect(),
                    _ => vec![],
                };
                if records.is_empty() {
                    Found::NoData
                } else {
                    Found::Records(records)
                }
            }
            ["_acme-challenge", label] => {
                let (base, door) = match label.strip_suffix(SHARE_SUFFIX) {
                    Some(base) => (base, true),
                    None => (*label, false),
                };
                let Some(e) = self.store.entry(base) else {
                    return Found::NxDomain;
                };
                let values = if e.acme_until <= now {
                    vec![]
                } else if door {
                    e.acme_door
                } else {
                    e.acme_box
                };
                if values.is_empty() {
                    return Found::NxDomain;
                }
                if qtype != RecordType::TXT {
                    return Found::NoData;
                }
                Found::Records(values.iter().map(|v| txt(owner, TTL_SHORT, v)).collect())
            }
            _ => Found::NxDomain,
        }
    }

    fn address_record(&self, rel: &str, owner: &Name, qtype: RecordType) -> Found {
        let Some(("_iroh", key)) = rel.split_once('.') else {
            return Found::NxDomain;
        };
        let Ok(key) = PublicKey::from_z32(key) else {
            return Found::NxDomain;
        };
        let Some(payload) = self.store.record(&key) else {
            return Found::NxDomain;
        };
        let Ok(packet) = SignedPacket::from_relay_payload(&key, &payload) else {
            return Found::NxDomain;
        };
        if qtype != RecordType::TXT {
            return Found::NoData;
        }
        let values = packet.txt_records("_iroh");
        if values.is_empty() {
            return Found::NoData;
        }
        Found::Records(values.iter().map(|v| txt(owner, TTL_RECORD, v)).collect())
    }

    fn zone_of(&self, qname: &str) -> &str {
        let q = qname.trim_end_matches('.');
        if q == self.zones.records || q.ends_with(&format!(".{}", self.zones.records)) {
            &self.zones.records
        } else {
            &self.zones.boxes
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for DnsHandler {
    async fn handle_request<R: ResponseHandler, T: Time>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let mut metadata = Metadata::response_from_request(&request.metadata);
        metadata.authoritative = true;
        metadata.recursion_available = false;
        let fallback = Header {
            metadata,
            counts: Default::default(),
        };
        let builder = || {
            let mut b = MessageResponseBuilder::from_message_request(request);
            if let Some(e) = request.edns.as_ref() {
                b.edns(e);
            }
            b
        };
        let send_empty = |mut handle: R, metadata: Metadata| async move {
            let response = builder().build_no_records(metadata);
            handle
                .send_response(response)
                .await
                .unwrap_or_else(|_| ResponseInfo::from(fallback))
        };

        if request.metadata.op_code != OpCode::Query {
            metadata.response_code = ResponseCode::NotImp;
            return send_empty(response_handle, metadata).await;
        }
        let Ok(info) = request.request_info() else {
            metadata.response_code = ResponseCode::FormErr;
            return send_empty(response_handle, metadata).await;
        };
        if request.protocol() == Protocol::Udp && !self.limiter.allow(&subnet(request.src().ip())) {
            metadata.truncation = true;
            return send_empty(response_handle, metadata).await;
        }
        let qname = info.query.name().to_string();
        let qtype = info.query.query_type();
        let found = self.lookup(&qname, qtype);
        debug!(%qname, ?qtype, ?found, "dns");
        let (answers, soa) = match found {
            Found::Records(r) => (r, vec![]),
            Found::NoData => (vec![], self.soa(self.zone_of(&qname)).into_iter().collect()),
            Found::NxDomain => {
                metadata.response_code = ResponseCode::NXDomain;
                (vec![], self.soa(self.zone_of(&qname)).into_iter().collect())
            }
            Found::Refused => {
                metadata.authoritative = false;
                metadata.response_code = ResponseCode::Refused;
                (vec![], vec![])
            }
        };
        // Answer with a modest size, so big answers go over TCP.
        let mut edns = Edns::new();
        edns.set_max_payload(1232);
        let mut b = MessageResponseBuilder::from_message_request(request);
        if request.edns.is_some() {
            b.edns(&edns);
        }
        let response = b.build(metadata, answers.iter(), [].iter(), soa.iter(), [].iter());
        response_handle
            .send_response(response)
            .await
            .unwrap_or_else(|_| ResponseInfo::from(fallback))
    }
}

/// Serves DNS on UDP and TCP until the program stops.
pub async fn serve(bind: SocketAddr, handler: DnsHandler) -> Result<SocketAddr> {
    let mut server = hickory_server::Server::new(handler);
    let udp = UdpSocket::bind(bind)
        .await
        .with_context(|| format!("DNS on UDP {bind}"))?;
    let addr = udp.local_addr()?;
    server.register_socket(udp);
    let tcp = TcpListener::bind(addr)
        .await
        .with_context(|| format!("DNS on TCP {addr}"))?;
    server.register_listener(tcp, Duration::from_secs(5), 64 * 1024);
    info!("DNS on {addr}");
    tokio::spawn(async move {
        if let Err(err) = server.block_until_done().await {
            tracing::error!("DNS stopped: {err}");
        }
    });
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Rules, Store};
    use zoomriya_keys::SecretKey;
    use zoomriya_proto::{AcmeTarget, HomeAddrs, RegisterRequest, names::NameRules};

    fn zones() -> Zones {
        Zones {
            boxes: "zoomriya.app".into(),
            records: "dns.zoomriya.net".into(),
            nameservers: vec!["ns1.zoomriya.net".into(), "ns2.zoomriya.net".into()],
            hostmaster: "hostmaster.zoomriya.net".into(),
            certificate_authorities: vec!["letsencrypt.org".into(), "pki.goog".into()],
        }
    }

    fn handler() -> (DnsHandler, Arc<Store>, SecretKey) {
        let store = Arc::new(Store::in_memory().expect("store"));
        let key = SecretKey::generate();
        store
            .register(
                &RegisterRequest {
                    name: "maple".into(),
                    box_key: key.public().into(),
                    dns_update_key: SecretKey::generate().public().into(),
                    home: HomeAddrs {
                        ipv4: vec!["192.168.1.20".parse().expect("ip")],
                        ipv6: vec!["fd00::20".parse().expect("ip")],
                    },
                    factory_token: None,
                },
                "81.2.69.160".parse().expect("ip"),
                zoomriya_serve::now(),
                &Rules {
                    names: NameRules::new(),
                    per_address_per_day: 10,
                    released_name_days: 90,
                },
            )
            .expect("registers");
        (DnsHandler::new(store.clone(), zones(), 50), store, key)
    }

    fn count(f: Found) -> usize {
        match f {
            Found::Records(r) => r.len(),
            other => panic!("expected records, got {other:?}"),
        }
    }

    #[test]
    fn box_names() {
        let (h, store, _) = handler();
        assert_eq!(count(h.lookup("maple.zoomriya.app.", RecordType::A)), 1);
        assert_eq!(count(h.lookup("MAPLE.zoomriya.app", RecordType::AAAA)), 1);
        assert_eq!(
            h.lookup("maple.zoomriya.app", RecordType::MX),
            Found::NoData
        );
        assert_eq!(h.lookup("oak.zoomriya.app", RecordType::A), Found::NxDomain);
        assert_eq!(
            h.lookup("a.maple.zoomriya.app", RecordType::A),
            Found::NxDomain
        );
        assert_eq!(h.lookup("example.com", RecordType::A), Found::Refused);
        assert_eq!(
            h.lookup("zoomriya.app.evil.com", RecordType::A),
            Found::Refused
        );

        // The door: only while open.
        assert_eq!(
            h.lookup("maple-share.zoomriya.app", RecordType::A),
            Found::NoData
        );
        store
            .set_door(
                "maple",
                Some("81.2.69.160".parse().expect("ip")),
                None,
                zoomriya_serve::now(),
            )
            .expect("door");
        assert_eq!(
            count(h.lookup("maple-share.zoomriya.app", RecordType::A)),
            1
        );
        assert_eq!(
            h.lookup("maple-share.zoomriya.app", RecordType::AAAA),
            Found::NoData
        );

        // Certificate checks.
        assert_eq!(
            h.lookup("_acme-challenge.maple.zoomriya.app", RecordType::TXT),
            Found::NxDomain
        );
        store
            .set_acme(
                "maple",
                AcmeTarget::Box,
                &["token-1".into(), "token-2".into()],
                zoomriya_serve::now(),
            )
            .expect("acme");
        assert_eq!(
            count(h.lookup("_acme-challenge.maple.zoomriya.app", RecordType::TXT)),
            2
        );
        assert_eq!(
            h.lookup("_acme-challenge.maple-share.zoomriya.app", RecordType::TXT),
            Found::NxDomain
        );
    }

    #[test]
    fn the_zone_itself() {
        let (h, _, _) = handler();
        assert_eq!(count(h.lookup("zoomriya.app", RecordType::NS)), 2);
        assert_eq!(count(h.lookup("zoomriya.app", RecordType::SOA)), 1);
        match h.lookup("zoomriya.app", RecordType::CAA) {
            Found::Records(r) => {
                let text: Vec<String> = r.iter().map(|r| r.data.to_string()).collect();
                assert_eq!(
                    text,
                    ["0 issue \"letsencrypt.org\"", "0 issue \"pki.goog\""]
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(h.lookup("zoomriya.app", RecordType::A), Found::NoData);
        assert_eq!(count(h.lookup("dns.zoomriya.net", RecordType::NS)), 2);
        assert_eq!(h.lookup("dns.zoomriya.net", RecordType::CAA), Found::NoData);
    }

    #[test]
    fn address_records() {
        let (h, store, key) = handler();
        let qname = format!("_iroh.{}.dns.zoomriya.net", key.public().to_z32());
        assert_eq!(h.lookup(&qname, RecordType::TXT), Found::NxDomain);
        let packet = SignedPacket::from_txt_strings(
            &key,
            "_iroh",
            ["relay=https://relay1.zoomriya.net./"],
            30,
        )
        .expect("packet");
        store
            .put_record(&key.public(), &packet.to_relay_payload())
            .expect("stores");
        match h.lookup(&qname, RecordType::TXT) {
            Found::Records(r) => {
                assert_eq!(r.len(), 1);
                assert!(
                    r[0].to_string()
                        .contains("relay=https://relay1.zoomriya.net./")
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(h.lookup(&qname, RecordType::A), Found::NoData);
        assert_eq!(
            h.lookup("_iroh.nonsense.dns.zoomriya.net", RecordType::TXT),
            Found::NxDomain
        );
    }

    #[test]
    fn subnets() {
        assert_eq!(
            subnet("81.2.69.160".parse().expect("ip")),
            "81.2.69.0".parse::<IpAddr>().expect("ip")
        );
        assert_eq!(
            subnet("2a01:4f8:1:2a77::5".parse().expect("ip")),
            "2a01:4f8:1:2a00::".parse::<IpAddr>().expect("ip")
        );
    }
}
