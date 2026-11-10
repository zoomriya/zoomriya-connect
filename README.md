# ZOOMRIYA Connect

ZOOMRIYA Connect is designed to let you reach your home box from your phone
or laptop, and to let guests open albums you share with them. Paired devices
connect directly to the box when possible. If direct paths fail, an optional
encrypted relay can carry the connection, avoiding manual router setup.

Family data normally travels directly between the paired device and the box.
When direct connection fails, an explicitly configured relay can forward the
encrypted traffic; with the planned end-to-end encryption, the relay is not
intended to read the application data.

**Status:** early development. The design is in place, but the code is still
a skeleton. The features described here are not yet available.

The design uses [iroh](https://github.com/n0-computer/iroh) for connections
between family devices. It prefers direct paths and can use a configured
relay as a fallback. Guests use WebRTC from their browser, with direct
connection first and an optional relay fallback.

![Phones and laptops reach the box directly when possible, or through an optional encrypted relay if direct paths fail. Guests open read-only album shares directly or through an optional relay. The control plane handles DNS and connection setup; the relay is a separate service and is intended to forward ciphertext without reading it.](docs/data-flow.svg)

## What Connect is designed to do

1. **Direct connection first; relay fallback.** The phone and desktop apps
   try local, IPv6 and NAT-traversed paths first. If those fail, an optional
   ZOOMRIYA-operated or customer-operated relay can carry encrypted traffic.
   Each paired device and the box check each other's keys.
2. **Laptops can use their regular browser.** A desktop app for Linux,
   Windows and macOS provides the connection, so a browser can open the box's
   own HTTPS address.
3. **The box's sign-in and apps stay private.** They are not served directly
   to the public internet. At home, any browser can open the box's address,
   such as `https://maplehouse.zoomriya.app`.
4. **Guests can view shared albums in a browser.** They do not need an
   account or an app, and access is read-only.
5. **Router setup is normally automatic.** When a guest link needs an
   internet-facing route, the box tries to open one itself if the router
   allows it.
6. **Some networks need extra setup or may not work.** For example, an
   internet provider may share one public address among many homes. Advanced
   users may be able to configure a port forward, but it cannot bypass every
   provider restriction.
7. **Relays are a fallback, not the normal path.** A relay is used only when
   direct connection attempts fail and relay access is enabled. It forwards
   encrypted traffic and can observe connection metadata such as IP
   addresses, timing and volume, but not the content. The operator and any
   bandwidth or retention limits must be disclosed.

The box must connect to the router over Ethernet, not Wi-Fi. Setup checks the
connection and explains this requirement.

## How it works

For family members, the phone or desktop app pairs with the box and tries
direct paths first. The control plane helps the two ends find each other. If
direct paths fail, the client can use the configured relay fallback. Family
traffic is encrypted end to end and the box's sign-in page and apps are not
exposed directly to the internet. Connect does not provide access to other
devices on the home network.

Guests use a separate, read-only route to the albums selected by the owner.
The box opens an internet-facing route only while a share link is active;
when that is not possible, the guest's browser tries WebRTC hole punching.
If direct paths still fail, an optional WebRTC relay can forward the
encrypted share traffic. The relay can carry only the share session, not
the box's dashboard or other apps.

For the network details and connection diagrams, see [How it works](docs/how-it-works.md).

![Family devices try direct paths first and use an encrypted relay if all fail. Guests load a page from share.zoomriya.net, then try the box directly or by WebRTC hole punching, with an optional relay fallback for the read-only album session.](docs/paths.svg)

## Where it works

- **At home,** always, from any browser.
- **Away from home, with the app:** designed to work on most networks without
  setup. Direct paths may fail behind strict NAT or blocked UDP; the relay
  fallback is intended to keep paired-device access working.
- **For guests:** a direct connection is preferred when the box can open a
  route through its router. Otherwise, the browser tries WebRTC and, if
  enabled, a relay fallback. The owner is told when a share will use a relay.

The full tables for different home and guest networks are in
[Where it works](docs/where-it-works.md).

## Read more

| Document | What's in it |
|---|---|
| [How it works](docs/how-it-works.md) | Connection mechanisms, data paths, box names, diagrams, and components |
| [Where it works](docs/where-it-works.md) | Home internet types, what works on each setup, guests' networks, what never works |
| [Desktop app](docs/desktop.md) | The local proxy and PAC file, autostart, key storage, packaging |
| [Sharing, names and certificates](docs/sharing-and-names.md) | Guest links, the order of attempts, box names and domains, certificates |
| [Security model](docs/security.md) | What each party can and can't do if compromised |
| [Plan and layout](docs/plan.md) | Build phases and the code layout |

## Building

```
cargo build
cargo test
```

The Rust version is pinned in `rust-toolchain.toml`; `rustup` picks it up. Dependencies are checked with `cargo deny check` against the licence policy in `deny.toml`.

## Security

Please report vulnerabilities privately, as described in [SECURITY.md](SECURITY.md), never in a public issue.

## Licence

[AGPL-3.0-only](LICENSE-AGPL-3.0). The ZOOMRIYA name and logo are not covered by the licence: see [TRADEMARKS.md](TRADEMARKS.md).
