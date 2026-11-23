# ZOOMRIYA Direct+

Direct+ lets your family reach your ZOOMRIYA Box from anywhere, with **no
ZOOMRIYA server in between**: no relay, no VPS, no account. Only paired
devices get in, and everything between a device and the box is encrypted.

> **`direct+` branch.** This branch removes ZOOMRIYA Connect's servers (the
> control plane, the relay and their GUI), the box's code that talked to
> them, and the laptop browser extension. `main` keeps them.

## How it works, in short

| Step | What happens |
|---|---|
| Pairing, at home | Scan the QR code on the box with the ZOOMRIYA phone app, or type the short code into the ZOOMRIYA laptop app. The box adds the device to its own list. No internet needed |
| Connecting | The app connects to the box with iroh (QUIC), over the home network, IPv6, or a port the router opened, finding it through its sealed record in the BitTorrent DHT. When no direct path works, it reaches the box's own relay through Tor, and iroh moves to a direct path once punching works |
| Guests | With the family's own domain, guests open `share.home.<domain>:8443` and see the album viewer only |
| Who gets in | The box's own list of paired devices, checked on every path and every stream. Removing a device cuts it off at once |
| Laptops | The laptop app connects; the browser shows the box at `http://zoomriya.localhost:7480` |

## Documents

| Page | For |
|---|---|
| [How it works](docs/how-it-works.md) | Everyone: from pairing to the photos, away from home |
| [Where it works](docs/where-it-works.md) | Which homes and networks, and what Box Desktop says |
| [Sharing, names and certificates](docs/sharing-and-names.md) | Guests, and the family's own domain |
| [Security model](docs/security.md) | What protects what, the keys, the limits |
| [Architecture](docs/architecture.md) | Builders: the parts, connectd's modules, its API and files |
| [Testing](docs/testing.md) | Builders: tests on every change, and by hand |
| [Direct+](docs/direct-plus.md) | The design, its decisions, phases and open questions |

## What is here

| Path | What |
|---|---|
| `connectd/` | The box's side: pairing, the device list, the link, Box Desktop's API |
| `crates/keys` | Box and device keys |
| `crates/proto` | What the apps and connectd say to each other |
| `crates/link` | The apps' side: finding the box at home and in the DHT, pairing, the connection |
| `crates/tor` | Tor (Arti): the box's onion service in front of its own relay, and the apps' way to it |
| `laptop/` | The laptop app's core and its command line (`zoomriya-laptop`) |
| `laptop/app` | The laptop app: a tray app (Tauri) that only connects; the browser shows the box |
| `crates/punch`, `crates/ffi` | Punching and the phone app's bindings (later phases) |
| `deploy/connectd.toml` | connectd's settings on the box |

## Building

```
cargo build
cargo test
```

The Rust version is pinned in `rust-toolchain.toml`, and `rustup` picks it up.
`cargo deny check` checks dependencies against the licence rules in
`deny.toml`.

## Security

Please report security problems privately, as described in
[SECURITY.md](SECURITY.md), never in a public issue.

## Licence

[AGPL-3.0-only](LICENSE-AGPL-3.0). The ZOOMRIYA name and logo are not covered
by the licence: see [TRADEMARKS.md](TRADEMARKS.md).
