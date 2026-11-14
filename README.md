# ZOOMRIYA Connect

ZOOMRIYA Connect lets your family reach your ZOOMRIYA Box from anywhere: on
the bus, at work, on holiday. It also lets you share an album with friends,
who just open a link in their browser.

You don't set anything up on your router, and the box is never put on the open
internet. Everything between your phone and the box is encrypted, and only
devices you've paired can get in.

> **Early days.** Most of this isn't built yet. These pages describe what
> we're building and why, so you can follow along and tell us where we're
> wrong.

![Phones and laptops try the box directly and through ZOOMRIYA's relay at the same time, and switch to a direct path when one works. Guests open shared albums directly when they can, with the relay as the last resort. The control plane only helps devices find the box; it never sees your data.](docs/data-flow.svg)

## How it works, in short

**For your family.** You pair your phone with the box once, at home, by
scanning a QR code on the box's screen. After that, the ZOOMRIYA app on your
phone can reach the box wherever you are. On a laptop, the ZOOMRIYA browser
extension does the same.

When you open the app, it tries every way to the box at the same moment: your
home network, IPv6, a port you've forwarded, and our relay, a server that
passes messages along. Whichever answers first carries your data. At home, or
when both ends have IPv6, that's usually the direct way, and the relay never
carries anything. Otherwise the relay answers first, so the app still opens
quickly, and behind the scenes your phone and the box try to reach each other
directly. They usually manage within a few seconds. Once they do, the relay
steps aside. If they can't, say on a strict office network, the relay keeps
passing your data along. It's encrypted, so the relay can't read it.

The box also gives each paired phone a pass, which the relay checks before
letting it through. There's a step-by-step picture of all this in
[How it works](docs/how-it-works.md#connecting).

We build on [iroh](https://github.com/n0-computer/iroh), an open-source
library that works this way. We run our own relays instead of iroh's public
ones, and families who want to can run their own.

**For guests.** You make a share link for an album and send it. Your friend
opens it in any browser, with no app and no account, and can look at that
album and nothing else. While a share link is live, the box opens a door on
your router. Guests reach only the album viewer through it. Your own paired
devices can use it as a direct way in too, and the box turns away anyone
else.

**At home.** Any browser on your home network opens the box at its own
address, such as `https://maplehouse.zoomriya.app`, with a proper padlock.

**Without our servers.** Families can run ZOOMRIYA Connect's server side
themselves. Or, if they don't want to look after a server, the box can use
their own Cloudflare Tunnel instead. The same phone app and laptop extension
work with both. Then Cloudflare is always in the middle, and can see the photos in
shared albums. See [Four ways to reach your box](docs/sharing-and-names.md#four-ways-to-reach-your-box).

![Four ways to reach the box from outside: a ZOOMRIYA name, your own domain through ZOOMRIYA, your own ZOOMRIYA Connect server, or your own Cloudflare Tunnel. In every option nothing is set up on the router.](docs/options.svg)

![The order in which connections are tried. Family devices try the box's known addresses and the relay at the same moment, and the first to answer carries data; hole punching runs through the relay, and a direct path takes over when one works. Guests try the box directly, then a browser-to-box connection, then the relay.](docs/paths.svg)

## What you need

- **The box plugged into your router with a cable.** The box itself can't use
  Wi-Fi. Setup checks this.
- **The ZOOMRIYA app** on each phone, and the **ZOOMRIYA browser extension** on
  each laptop (Chrome, Edge or Firefox), if you want to use the box away from
  home.
- Nothing on the router, in most homes.

## Where it works

At home, always. Away from home, the app works on almost any network that
lets you browse the web, because you reach the relay the same way you reach a
website. A few networks block everything except approved sites. Those won't
work.

Guests can usually open a link. When your home can't accept a direct
connection and their network is strict too, the relay carries the album. If
even that is blocked, the page tells them so, and you're warned about this
when you make the link.

The details, network by network, are in [Where it works](docs/where-it-works.md).

## Privacy

- Your photos, messages and files travel encrypted between your device and
  your box. Nobody in between can read them: not your internet provider, not
  the relay, not us.
- The relay sees that a device is talking to a box, from which addresses,
  when, and how much. It doesn't see what. We'll say who runs it and what it
  keeps. You can switch it off, or run your own.
- Photo and video backups wait for a direct connection. They never go through
  our relay. On a relay of your own, a switch lets them.
- The box's sign-in page and apps are never on the open internet.
- With your own Cloudflare Tunnel, Cloudflare sees who connects and when, and
  the photos in shared albums. Family data stays sealed by the app and the extension.

What each part could and couldn't do if someone broke into it is in the
[security model](docs/security.md).

## Read more

| Page | What's in it |
|---|---|
| [How it works](docs/how-it-works.md) | Pairing, passes, connecting step by step, the box's name, and the parts |
| [Architecture](docs/architecture.md) | For advanced users: every part, the keys, how they work together, and what happens when one fails |
| [Where it works](docs/where-it-works.md) | Types of home internet, what works on each, guests' networks, what never works |
| [Laptops](docs/laptops.md) | The browser extension: how laptops reach the box in every option, and the checks it depends on |
| [Sharing, names and certificates](docs/sharing-and-names.md) | Share links, box names, the four ways to reach your box (your own domain, your own server, your own Cloudflare Tunnel), certificates |
| [Security model](docs/security.md) | What each part could and couldn't do if someone took it over |
| [Plan and layout](docs/plan.md) | For developers: what we build in which order, and where the code goes |
| [Running the service](docs/operations.md) | For whoever runs the servers: backups, alerts, being on call, protecting DNS, changing keys |
| [Testing at scale](docs/testing.md) | For developers: how we'll test 1,000 families over two months before release |

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

[AGPL-3.0-only](LICENSE-AGPL-3.0). The ZOOMRIYA name and logo aren't covered by
the licence: see [TRADEMARKS.md](TRADEMARKS.md).
