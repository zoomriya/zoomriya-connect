# ZOOMRIYA Connect

ZOOMRIYA Connect lets your family reach your ZOOMRIYA box from anywhere: on
the bus, at work, on holiday. It also lets you share an album with friends who
just open a link in their browser.

You don't set anything up on your router, and the box is never put on the open
internet. Everything between your phone and the box is encrypted, and only
devices you've paired can get in.

> **Early days.** Most of this isn't built yet. These pages describe what
> we're building and why, so you can follow along and tell us where we're
> wrong.

![Phones and laptops connect to the box through ZOOMRIYA's relay, then switch to a direct path when one works. Guests open shared albums directly when they can, with the relay as the last resort. The control plane only helps devices find the box; it never sees your data.](docs/data-flow.svg)

## How it works, in short

**For the family.** You pair your phone with the box once, at home, by
scanning a QR code on the box's screen. After that, the ZOOMRIYA app on your
phone or laptop can reach the box wherever you are.

When you open the app, it connects through our relay first, so it's quick.
Behind the scenes, your phone and the box then try to reach each other
directly, and usually manage within a few seconds. Once they do, your data
goes straight between them and the relay steps aside. If they can't, say on a
strict office network, the relay keeps passing the data along. It's still
encrypted, so the relay can't read it.

There's a step-by-step picture of this in [How it works](docs/how-it-works.md#connecting).

This is how [iroh](https://github.com/n0-computer/iroh), the open-source
library we build on, works. We run our own relays instead of using iroh's
public ones. Families who want to can run their own.

**For guests.** You make a share link for an album and send it. Your friend
opens it in any browser, with no app and no account, and can look at that
album and nothing else. The box opens a door on your router only while a share
link is live, and that door leads only to the album viewer.

**At home.** Any browser on your home network opens the box at its own address,
such as `https://maplehouse.zoomriya.app`, with a proper padlock.

![The order in which connections are tried. Family devices start on the relay and try six direct paths at once; the first that works takes over. Guests try the box directly, then a browser-to-box connection, then the relay.](docs/paths.svg)

## What you need

- **The box plugged into your router with a cable.** Wi-Fi isn't supported for
  the box itself. Setup checks this.
- **The ZOOMRIYA app** on each phone, and the desktop app on laptops if you want
  to use the box in your normal browser away from home.
- Nothing on the router, in most homes.

## Where it works

At home, always. Away from home, the app works on almost any network that
lets you browse the web, because the relay is reached over the same kind of
connection as a website. A few networks block everything except approved
sites; those won't work.

Guests can usually open a link. When your home can't accept a direct
connection and their network is strict too, the relay carries the album. If
even that is blocked, the page tells them so, and you're warned about this
when you make the link.

The details, network by network, are in [Where it works](docs/where-it-works.md).

## Privacy

- Your photos, messages and files travel encrypted between your device and
  your box. Nobody in the middle can read them: not your internet provider,
  not the relay, not us.
- The relay sees that a device is talking to a box, from which addresses,
  when, and how much. It doesn't see what. We'll say who runs it and what it
  keeps. You can switch it off, or run your own.
- Photo and video backups wait for a direct connection. They never go through
  our relay. On a relay of your own, a switch lets them.
- The box's sign-in page and apps are never on the open internet.

What each part could and couldn't do if it were broken into is in the
[security model](docs/security.md).

## Read more

| Page | What's in it |
|---|---|
| [How it works](docs/how-it-works.md) | Connections step by step, the box's name, and the parts of the system |
| [Where it works](docs/where-it-works.md) | Home internet types, what works on each, guests' networks, what never works |
| [Desktop app](docs/desktop.md) | How laptops use the box in a normal browser |
| [Sharing, names and certificates](docs/sharing-and-names.md) | Guest links, box names, your own domain, your own server, certificates |
| [Security model](docs/security.md) | What each part could and couldn't do if it were compromised |
| [Plan and layout](docs/plan.md) | What we're building in which order, and where the code goes |
| [Testing at scale](docs/testing.md) | How we'll test 1,000 families over two months before release |

## Building

```
cargo build
cargo test
```

The Rust version is pinned in `rust-toolchain.toml`; `rustup` picks it up.
`cargo deny check` checks dependencies against the licence policy in
`deny.toml`.

## Security

Please report vulnerabilities privately, as described in
[SECURITY.md](SECURITY.md), never in a public issue.

## Licence

[AGPL-3.0-only](LICENSE-AGPL-3.0). The ZOOMRIYA name and logo are not covered by
the licence: see [TRADEMARKS.md](TRADEMARKS.md).
