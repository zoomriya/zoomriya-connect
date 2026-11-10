# ZOOMRIYA Connect

ZOOMRIYA Connect is designed to let you reach your home box from your phone
or laptop, and to let guests open albums you share with them. Paired devices
start each connection through a relay, so it opens at once, then move to a
direct path to the box as soon as one works, usually within seconds. If no
direct path works, the relay keeps carrying the connection, so no router setup
is ever needed.

Family data is encrypted end to end between the paired device and the box.
The relay is run by ZOOMRIYA or by the family, never a third party, and
forwards that data without being able to read it.

**Status:** early development. The design is in place, but the code is still
a skeleton. The features described here are not yet available.

The design uses [iroh](https://github.com/n0-computer/iroh) for connections
between family devices, the way iroh is meant to be used: each connection
starts on a relay, which also passes the hole-punching signals, and moves to a
direct path when one works. Only our relays are used, never iroh's public
ones. Guests use WebRTC from their browser: direct first, with a relay as the
last option.

![Phones and laptops start each connection through the relay, then move to a direct path to the box when one works; the relay keeps carrying encrypted traffic only if none does. Guests open read-only album shares directly, with a relay as the last option. The control plane handles DNS and address records; the relay forwards ciphertext without reading it.](docs/data-flow.svg)

## What Connect is designed to do

1. **Relay first, then direct.** Each connection from the phone or desktop
   app starts through the relay, so it opens at once, then moves to a direct
   path (the home network, IPv6 or hole punching) as soon as one works. Only
   when none works does the relay keep carrying it. Each paired device and
   the box check each other's keys.
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
7. **The relay is ZOOMRIYA's or the family's.** Never a third party's. It
   admits only boxes and their paired devices and forwards encrypted traffic.
   It can see connection metadata such as IP addresses, timing and volume,
   but not the content. Backups wait for a direct path instead of using it.
   The operator and any bandwidth or retention limits are disclosed.

The box must connect to the router over Ethernet, not Wi-Fi. Setup checks the
connection and explains this requirement.

## How it works

For family members, the phone or desktop app pairs with the box. Each
connection starts through the relay and moves to a direct path when one
works; the control plane's signed address records tell the app which relay
the box uses. Family traffic is encrypted end to end and the box's sign-in page and apps are not
exposed directly to the internet. Connect does not provide access to other
devices on the home network.

Guests use a separate, read-only route to the albums selected by the owner.
The box opens an internet-facing route only while a share link is active;
when that is not possible, the guest's browser tries WebRTC hole punching.
If direct paths still fail, a WebRTC relay can forward the encrypted share
traffic as the last option. The relay can carry only the share session, not
the box's dashboard or other apps.

For the network details and connection diagrams, see [How it works](docs/how-it-works.md).

![Family devices start each connection through the relay, which passes the hole-punching signals; paths 1–6 are tried at once and the first that works takes over. Guests load a page from share.zoomriya.net, then try the box directly or by WebRTC hole punching, with a relay as the last option for the read-only album session.](docs/paths.svg)

## Where it works

- **At home,** always, from any browser.
- **Away from home, with the app:** designed to work on most networks without
  setup: on any network that lets the app reach the relay over HTTPS. Most
  connections then move to a direct path; behind strict NAT or blocked UDP
  they stay on the relay.
- **For guests:** a direct connection is preferred when the box can open a
  route through its router. Otherwise, the browser tries WebRTC and, as the
  last option, a relay. The owner is told when a share will use a relay.

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
| [Testing at scale](docs/testing.md) | How we test 1,000 families over two months before release |

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
