//! The box's record in the BitTorrent DHT (Mainline, BEP 44): the box
//! publishes where it can be reached, paired devices look it up
//! ([`zoomriya_keys::record`]). The DHT is run by volunteers; no ZOOMRIYA
//! server is involved, and the record is sealed and moves every day.

use std::{
    net::{SocketAddr, SocketAddrV4},
    time::Duration,
};

use anyhow::Result;
use mainline::{Dht as Mainline, MutableItem, SigningKey, async_dht::AsyncDht};
use tracing::debug;
use zoomriya_keys::record::{RecordSecret, day};

/// What the DHT saw of the box's own network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outside {
    /// The address the DHT's nodes saw the box's queries come from.
    pub ipv4: Option<SocketAddrV4>,
    /// Whether unsolicited packets fail to reach the box's DHT port.
    pub firewalled: bool,
}

/// A DHT client.
#[derive(Debug, Clone)]
pub struct Dht(AsyncDht);

/// How long one lookup may take.
const LOOKUP: Duration = Duration::from_secs(15);

impl Dht {
    /// A client on the public DHT.
    pub fn start() -> Result<Self> {
        Ok(Self(Mainline::client()?.as_async()))
    }

    /// A client on another DHT, such as a test network on this machine.
    pub fn with_bootstrap(bootstrap: &[String]) -> Result<Self> {
        Ok(Self(
            Mainline::builder()
                .bootstrap(bootstrap)
                .bind_address(std::net::Ipv4Addr::LOCALHOST)
                .build()?
                .as_async(),
        ))
    }

    /// What the DHT saw of this network.
    pub async fn outside(&self) -> Outside {
        let info = self.0.info().await;
        Outside {
            ipv4: info.public_address(),
            firewalled: info.firewalled(),
        }
    }

    /// Publishes the record for today and tomorrow, so it is there across
    /// midnight and for clocks a little off.
    pub async fn publish(
        &self,
        secret: &RecordSecret,
        addrs: &[SocketAddr],
        now: u64,
    ) -> Result<()> {
        let value = secret.seal(addrs, now)?;
        let mut published = 0;
        for d in [day(now), day(now) + 1] {
            let key = SigningKey::from_bytes(&secret.signing_seed(d));
            let item = MutableItem::new(key, &value, now as i64, None);
            match self.0.put_mutable(item, None).await {
                Ok(_) => published += 1,
                Err(e) => debug!("publishing for day {d}: {e}"),
            }
        }
        anyhow::ensure!(published > 0, "the DHT took no record");
        Ok(())
    }

    /// The newest record from yesterday, today or tomorrow: its addresses
    /// and when the box wrote it.
    pub async fn lookup(&self, secret: &RecordSecret, now: u64) -> Option<(Vec<SocketAddr>, u64)> {
        let today = day(now);
        let tries = [today, today.saturating_sub(1), today + 1].map(|d| {
            let key = SigningKey::from_bytes(&secret.signing_seed(d))
                .verifying_key()
                .to_bytes();
            let dht = self.0.clone();
            async move { dht.get_mutable_most_recent(&key, None).await }
        });
        let [a, b, c] = tries;
        let found = tokio::time::timeout(LOOKUP, async { tokio::join!(a, b, c) })
            .await
            .ok()?;
        [found.0, found.1, found.2]
            .into_iter()
            .flatten()
            .filter_map(|item| secret.open(item.value()).ok())
            .max_by_key(|(_, at)| *at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_box_publishes_and_a_device_finds_it() {
        let net = mainline::Testnet::new_async(5).await.expect("testnet");
        let boxside = Dht::with_bootstrap(&net.bootstrap).expect("box");
        let device = Dht::with_bootstrap(&net.bootstrap).expect("device");
        let secret = RecordSecret::generate().expect("secret");
        let now = 1_800_000_000;
        let addrs: Vec<SocketAddr> = vec![
            "[2001:db8::7]:443".parse().expect("addr"),
            "203.0.113.7:443".parse().expect("addr"),
        ];
        boxside
            .publish(&secret, &addrs, now)
            .await
            .expect("published");
        assert_eq!(
            device.lookup(&secret, now + 60).await,
            Some((addrs.clone(), now))
        );
        // The next day still finds it (published for tomorrow too).
        assert_eq!(
            device.lookup(&secret, now + 86_400).await.map(|r| r.1),
            Some(now)
        );
        // A newer record wins.
        let moved: Vec<SocketAddr> = vec!["203.0.113.8:443".parse().expect("addr")];
        boxside
            .publish(&secret, &moved, now + 120)
            .await
            .expect("published");
        assert_eq!(
            device.lookup(&secret, now + 180).await,
            Some((moved, now + 120))
        );
        // Another box's secret finds nothing.
        let other = RecordSecret::generate().expect("secret");
        assert_eq!(device.lookup(&other, now).await, None);
    }
}
