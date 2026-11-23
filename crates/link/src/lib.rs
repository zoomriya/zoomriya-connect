//! The apps' side of the link, shared by the laptop app and the phone app:
//! finding the box on the home network, pairing, and streams to the box over
//! iroh, pinned to the box key. No ZOOMRIYA server is involved
//! (docs/direct-plus.md).
//!
//! | Module | What |
//! |---|---|
//! | [`find`] | The boxes announcing themselves on the home network |
//! | [`device`] | The device key, which vouches for the link key |
//! | [`pair`] | Pairing a laptop with the short code |
//! | [`client`] | The connection to a paired box, and streams to its web servers |

pub mod client;
pub mod device;
pub mod find;
pub mod pair;

use iroh::{Endpoint, RelayMode, endpoint::presets};
use zoomriya_keys::SecretKey;

/// An iroh endpoint for talking to the box: the device's link key, no relay.
pub async fn endpoint(link: SecretKey) -> anyhow::Result<Endpoint> {
    Ok(Endpoint::builder(presets::Minimal)
        .secret_key(link)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?)
}
