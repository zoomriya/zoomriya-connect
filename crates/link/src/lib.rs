//! The apps' side of the link, shared by the laptop app and the phone app:
//! finding the box on the home network, pairing, and streams to the box over
//! iroh, pinned to the box key. No ZOOMRIYA server is involved
//! (docs/direct-plus.md).
//!
//! | Module | What |
//! |---|---|
//! | [`find`] | The boxes announcing themselves on the home network |
//! | [`dht`] | The box's sealed record in the BitTorrent DHT, for finding it from away |
//! | [`device`] | The device key, which vouches for the link key |
//! | [`pair`] | Pairing a laptop with the short code |
//! | [`client`] | The connection to a paired box, and streams to its web servers |
//! | [`endpoint_via`] | The endpoint reaching the box's own relay through Tor |

pub mod client;
pub mod device;
pub mod dht;
pub mod find;
pub mod pair;

use iroh::{Endpoint, RelayMap, RelayMode, RelayUrl, endpoint::presets};
use zoomriya_keys::SecretKey;

/// An iroh endpoint for talking to the box: the device's link key, no relay.
pub async fn endpoint(link: SecretKey) -> anyhow::Result<Endpoint> {
    Ok(Endpoint::builder(presets::Minimal)
        .secret_key(link)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?)
}

/// The box's relay through Tor: its address for iroh, and the endpoint
/// reaching it through `proxy`, a [`zoomriya_tor::proxy`] for that onion
/// address only.
pub async fn endpoint_via(
    link: SecretKey,
    onion: &str,
    proxy: std::net::SocketAddr,
) -> anyhow::Result<(Endpoint, RelayUrl)> {
    anyhow::ensure!(onion_ok(onion), "not an onion address");
    let relay: RelayUrl = format!("http://{onion}").parse()?;
    let proxy = url::Url::parse(&format!("http://{proxy}"))?;
    let ep = Endpoint::builder(presets::Minimal)
        .secret_key(link)
        .relay_mode(RelayMode::Custom(RelayMap::from(relay.clone())))
        .proxy_url(proxy)
        .bind()
        .await?;
    Ok((ep, relay))
}

/// Whether `s` is a version 3 onion address: 56 letters and digits, then
/// `.onion`.
pub fn onion_ok(s: &str) -> bool {
    s.strip_suffix(".onion").is_some_and(|name| {
        name.len() == 56
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn onion_addresses() {
        let good = format!("{}.onion", "a".repeat(55) + "d");
        assert!(super::onion_ok(&good));
        assert!(!super::onion_ok("example.com"));
        assert!(!super::onion_ok(&format!("{}.onion", "a".repeat(55))));
        assert!(!super::onion_ok(&format!("{}.onion", "A".repeat(56))));
        assert!(!super::onion_ok(&format!("x.{}.onion", "a".repeat(56))));
    }
}
