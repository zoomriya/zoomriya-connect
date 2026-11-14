# Where it works

Part of [ZOOMRIYA Connect](../README.md).

Short answer: at home, always. Away from home, on nearly any network where you
can browse the web. This page explains the exceptions.

We haven't tested this on real networks yet. It's what we expect.

## Plug the box in with a cable

The box must be connected to your router with an Ethernet cable, not Wi-Fi.
A cable is more reliable, and some Wi-Fi setups stop devices from talking to
each other. Setup checks for the cable.

Setup also checks that the box is on the same network as your Wi-Fi. Some
routers from internet providers, and some mesh systems, put certain sockets on
a separate guest or TV network.

## A few words you'll see

- **Public address.** Your home's address on the internet. Many homes have
  their own. Some providers share one address between many homes. That's
  called **CGNAT**, and it's common with mobile, 5G and fixed-wireless
  internet.
- **Port forward.** A rule on your router that lets connections from the
  internet reach one device at home. It only helps if your home has its own
  public address.
- **Hole punching.** Two devices, each behind a router, reach out to each
  other at the same moment, so both routers let the connection through.
- **Easy and strict routers.** Most routers allow hole punching. Strict ones
  (called **symmetric NAT**) stop it most of the time.
- **Port 443.** The port every secure website uses. Almost every network lets
  it through, because blocking it would break the web. That's why the relay
  and the guests' door use it.
- **UDP.** The kind of traffic hole punching uses. Some offices and hotels
  block it. TCP is the other kind. Websites normally use TCP.
- **The relay.** Our server, tried alongside the direct ways. It helps them
  get through, and carries the connection when nothing direct works. You
  reach it the same way you reach a website, so it gets through almost
  everywhere.

## By your home's internet

"App" means the ZOOMRIYA app on a phone, or the browser extension on a laptop, away from home. "Guest"
means someone opening a share link.

| Your home's internet | At home | App, easy network | App, strict network | App, UDP blocked | Guest |
|---|---|---|---|---|---|
| Its own public address, easy router | Works | Direct | Usually direct | Relay | Direct |
| Its own public address, strict router | Works | Usually direct | Relay | Relay | Direct if the box can open a door; otherwise WebRTC, then the relay |
| Shared address (CGNAT), easy | Works | Direct | Usually direct | Relay | WebRTC, then the relay |
| Shared address (CGNAT), strict | Works | Usually direct | Relay | Relay | WebRTC, then the relay |
| Home and device both have IPv6 | Works | Usually direct | Usually direct | Relay | Direct for guests with IPv6 |
| You forwarded port 443 (UDP and TCP) to the box | Works | Direct | Direct | Direct, over TCP | Direct |
| Remote access switched off | Works | No | No | No | No |
| Box on Wi-Fi | Not supported. Use a cable | | | | |

Laptops with the browser extension get the same, except over a port you
forwarded: the extension can only use the UDP one.

**Direct** means your data goes straight between the device and the box.
**Relay** means it works through our relay, a bit slower.

A port forward can't help if your provider shares your address (CGNAT). The
fix there is IPv6, or asking your provider for an address of your own. The
app still works through the relay either way.

## Guests

Guests never have to accept connections, only make them. So when your home
can open a door for them, they can come in from anywhere: mobile data, hotel
Wi-Fi, an office.

When your home can't open a door:

- **If your home has IPv6,** guests who also have IPv6 come straight in. Many
  hotels and offices don't have it.
- **Otherwise, their browser tries WebRTC.** That works if at least one of the
  two networks is an easy one. A guest on mobile data is more likely to be on
  a strict one.
- **If that fails, the relay carries the album.** That works anywhere the
  guest can browse normally.

When you make a link, the box tells you what to expect: "Guests can open this
link", "Guests may have trouble on some networks", or "Guests can't open links
from this home".

## With your own Cloudflare Tunnel

With [option 4](sharing-and-names.md#your-own-cloudflare-tunnel), everything
from outside goes through Cloudflare, so your home's internet doesn't matter.

| | Works? |
|---|---|
| Any home internet, shared address (CGNAT) included | Yes. The box dials out, so nothing needs opening |
| A laptop with the extension, on any network that lets you browse | Yes, through Cloudflare |
| A laptop with the extension, at home | Yes, straight to the box |
| A phone with the ZOOMRIYA app, on any network that lets you browse | Yes, through Cloudflare. Backups too, except files over 90 MB, which wait until you're home ([why](sharing-and-names.md#cloudflares-limits)) |
| A phone with the ZOOMRIYA app, at home | Yes, straight to the box |
| Guests | Yes, on any network that lets them browse |
| Cloudflare or your account down | Nothing from outside. Everything at home keeps working |
| The box on Wi-Fi | Not supported. Use a cable |

## Backups away from home

On Android, the app can back up in the background, once the connection is
direct.

On an iPhone, background uploads are handed to the phone's system, which
can't use ZOOMRIYA Connect. So backups may wait until you open the app or get
home. iPhones sometimes give apps a short time in the background, and we
still need to test how far that gets.

## Where it won't work

- Networks that only allow approved websites.
- Networks that open and inspect encrypted traffic. The app refuses them on
  purpose, because it checks the box's key.
- Hotel or café Wi-Fi before you've signed in to it.
- Any network that blocks our relay, when nothing direct works either. The
  same goes if you've switched the relay off.

For guests, the page explains when an album can't be opened from their
network, and you're warned when you make the link.
