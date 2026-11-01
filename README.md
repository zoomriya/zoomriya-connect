# ZOOMRIYA Connect

Private, direct connections to your ZOOMRIYA home box, from your phone, your laptop or a guest's browser. No relay, no manual port forwarding in the normal case, keys pinned on both ends for paired devices.

ZOOMRIYA Connect lets a family reach their ZOOMRIYA box at home from away, straight to the box. Everything the family keeps on the box goes only between the device and the box: nothing in between can read it, including ZOOMRIYA.

**Status:** early. The design is settled; the code is a skeleton. Nothing here works yet.

Family connections are built on [iroh](https://github.com/n0-computer/iroh), with its relay turned off and ZOOMRIYA's own signaling. Guests use WebRTC from the browser.

![Phones and laptops reach the box by hole punching, straight through the home router to connectd, which hands them to the box's apps. Guests opening a shared link reach the box's read-only share view directly, through a port the box opened on the router by itself or an IPv6 pinhole, or by hole punching from the browser. The control plane only swaps addresses, answers DNS for box names and hosts certificate challenges.](docs/architecture.svg)

## What we want

1. **No relay.** Nothing ZOOMRIYA runs carries family or guest traffic; the control plane does signaling and DNS only. Guests never use a relay of any kind. If a relay is ever added for family devices, it will be the last resort after every direct path, run on ZOOMRIYA's or the family's own servers, carry only end-to-end encrypted traffic and never backups or bulk uploads, and this README will say so first.
2. **Phones connect with no router setup.** Hole punching through the control plane, with pinned keys on both ends.
3. **Laptops connect with no router setup.** A desktop app for Linux, Windows and macOS runs the same link as the phone app, starts by itself at sign-in, and lets any browser open the box's own https address.
4. **Away from home, the box opens only in the apps.** The box's sign-in page and apps are never served to the internet. At home, any browser opens `https://maplehouse.zoomriya.app` with a real certificate.
5. **Guests can open a shared link** to an album, read-only, with nothing installed and no account.
6. **No manual port forwarding for normal users.** The box opens what it needs by itself when the router allows it.
7. **Advanced users with hard networks** (carrier-grade NAT, symmetric NAT on both ends) can set up a port forward by hand where the ISP allows it.

The box must be wired to the router by Ethernet, not Wi-Fi; the setup checks.

## How it works

- **Direct, by key.** Phones, laptops and the box each run an iroh endpoint, which connects by key instead of by address. They reach the box by hole punching: the control plane tells each side where the other is and when to dial. Paired devices need no open port on the router.
- **Pinned keys.** At home, each phone scans a QR code from the box, so each learns the other's key; a key in the phone's secure hardware signs for every connection. Only paired devices get in, and nothing in between can pose as the box.
- **Only the box, never the internet at large.** A connection reaches the box's own apps, never other devices at home; it isn't a VPN. The box's sign-in page and apps are never served to the internet.
- **Guests, read-only.** While a share link is live, the box opens port 443 on the router for a share listener that serves only that album. Otherwise the guest's browser hole punches by WebRTC. Guests never use a relay.
- **Only the servers you can see.** iroh's built-in public servers and its router port mapping are off, and an allow list blocks any other host. Devices talk only to the box and the control plane, ZOOMRIYA's or your own.
- **Helped, not carried.** The control plane finds the box, coordinates hole punching, answers DNS for box names and helps get the box's Let's Encrypt certificate. It stores no family data and can't make a device trusted.

![Family devices try six direct paths together: the home network, IPv6, hole punching, harder hole punching, the open path on UDP 443, and TLS over TCP 443; the first that authenticates wins. A relay, off for now, would come only after all six fail, never for backup or bulk uploads. Guests load a page from share.zoomriya.net, then reach the box directly over HTTPS through its share name, or by WebRTC hole punching; if neither works, the owner can send the photos instead. Guests never use a relay.](docs/paths.svg)

## Where it works

- **At home,** always, from any browser.
- **Away, with the app,** from most networks with no setup. It fails when both ends sit behind strict NAT (harder hole punching rescues some) or a network blocks UDP, unless the box has an open path.
- **Guests** get in directly when the box could open a port; otherwise by WebRTC, which fails in the same cases. The owner is told which when making the link.

The full tables, by home setup and by guest network, are in [Where it works](docs/where-it-works.md).

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

The Rust version is pinned in `rust-toolchain.toml`; `rustup` picks it up. Dependencies are checked with `cargo deny check`: only licences that combine with MPL-2.0 and the app stores are allowed.

## Security

Please report vulnerabilities privately, as described in [SECURITY.md](SECURITY.md), never in a public issue.

## Licence

[MPL-2.0](LICENSE). Changes to these files stay open; you may combine them with your own code under its own terms. The ZOOMRIYA name and logo are not covered by the licence: see [TRADEMARKS.md](TRADEMARKS.md).
