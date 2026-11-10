# ZOOMRIYA Connect

ZOOMRIYA Connect is designed to let you reach your home box from your phone
or laptop, and to let guests open albums you share with them. Paired devices
connect directly to the box in the normal case, without a relay or router
setup.

Family data travels between the paired device and the box. The services that
help the devices find each other do not carry that data and cannot read it.

**Status:** early development. The design is in place, but the code is still
a skeleton. The features described here are not yet available.

The design uses [iroh](https://github.com/n0-computer/iroh) for connections
between family devices, with its relay service disabled. Guests use WebRTC
from their browser.

![Phones and laptops reach the box by hole punching, straight through the home router to connectd, which hands them to the box's apps. Guests opening a shared link reach the box's read-only share view directly, through a port the box opened on the router by itself or an IPv6 pinhole, or by hole punching from the browser. The control plane only swaps addresses, answers DNS for box names and hosts certificate challenges.](docs/architecture.svg)

## What Connect is designed to do

1. **Family devices connect directly.** The phone and desktop apps are
   designed to connect to the box without a relay or, in most homes, changes
   to the router. Each paired device and the box check each other's keys.
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
7. **No relay carries family or guest traffic.** The control plane helps
   devices find each other but does not carry their connections. If a relay
   is ever considered for family devices, it would be a last resort, run by
   ZOOMRIYA or the family, and disclosed here before use. Guests will not use
   a relay.

The box must connect to the router over Ethernet, not Wi-Fi. Setup checks the
connection and explains this requirement.

## How it works

For family members, the phone or desktop app pairs with the box and tries to
connect directly. The control plane helps the two ends find each other but
does not carry their connection. The box's sign-in page and apps are not
exposed directly to the internet, and Connect does not provide access to
other devices on the home network.

Guests use a separate, read-only route to the albums selected by the owner.
The box opens an internet-facing route only while a share link is active;
when that is not possible, the guest's browser tries to connect directly
using WebRTC. Guests do not use a relay.

For the network details and connection diagrams, see [How it works](docs/how-it-works.md).

![Family devices try six direct paths together: the home network, IPv6, hole punching, harder hole punching, the open path on UDP 443, and TLS over TCP 443; the first that authenticates wins. A relay, off for now, would come only after all six fail, never for backup or bulk uploads. Guests load a page from share.zoomriya.net, then reach the box directly over HTTPS through its share name, or by WebRTC hole punching; if neither works, the owner can send the photos instead. Guests never use a relay.](docs/paths.svg)

## Where it works

- **At home,** always, from any browser.
- **Away from home, with the app:** designed to work on most networks without
  setup. It may not connect if both ends are behind strict network address
  translation (NAT), or if a network blocks UDP, unless the box has another
  route open.
- **For guests:** a direct connection is preferred when the box can open a
  route through its router. Otherwise, the browser tries WebRTC, which has
  similar network limitations. The owner is told what to expect when creating
  a link.

The full tables for different home and guest networks are in
[Where it works](docs/where-it-works.md).

## Read more

| Document | What's in it |
|---|---|
| [How it works](docs/how-it-works.md) | Every mechanism in detail, the link diagram, and the parts |
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
