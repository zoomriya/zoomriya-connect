# Plan and layout

Part of [ZOOMRIYA Connect](../README.md).

## What we build, in order

1. **[Give each box a name](sharing-and-names.md#names-and-domains).** Build
   `connectd`, the box's side, on port 443. Get the box a Let's Encrypt
   certificate through DNS, set up DNS for box names, publish the box's
   address, and check it's plugged in with a cable.
2. **[Get ready for guests](where-it-works.md#guests).** Check whether the
   home has working IPv6. Try opening a port on the router automatically. Test
   from outside whether it worked. When it doesn't, explain why in plain words
   and show the steps for common routers. Let owners switch guest access off.
3. **[Build the family link](how-it-works.md).**
   - Run our own iroh relays, with iroh's public ones switched off. They let
     in only boxes and their paired devices, and have address discovery
     switched on (`enable_quic_addr_discovery`, which is off by default).
   - Run our own address server (`iroh-dns-server`).
   - Build pairing, the link and the desktop proxy. Keep each device's iroh
     key locked by its secure chip or key store, and keep backups off our
     relay.
   - Bring it all to Android and iOS.
   - Add cleverer hole punching only if measurements show we need it.
4. **[Build the desktop app](desktop.md).** Start with a plain Linux proxy for
   testing. Then add the tray icon, starting at sign-in, and the browser setup
   for Linux, Windows and macOS. Sign the installers.
5. **[Add share links](sharing-and-names.md#sharing-with-guests).** Tokens,
   the guest page, direct connections and WebRTC, with the relay as the last
   resort. Give owners their controls, and tell them what to expect.
6. **Help advanced homes.** Manual port forwards and IPv6 firewall rules.
   For families who forward port 443, a direct path over TCP for networks
   that block UDP. iroh doesn't do this, so it's our own: TLS with the same
   key checks, sharing TCP 443 with the guests' album viewer (the TLS
   handshake says which one a connection is for). The box turns it on only
   when its outside check finds the forward working, and the app uses it
   only when UDP is blocked. It matters most to families who switch the
   relay off.
7. **[Own domains](sharing-and-names.md#names-and-domains).** A CNAME from the
   family's domain to their ZOOMRIYA name. Dynamic DNS from other companies
   isn't supported.
8. **[Test at scale](testing.md).** Simulate 1,000 families for two months.
   How many connections go direct, how fast they open, what the relay carries
   and costs, what breaks. Then fix the most common failures first.
9. **Get ready for release.** Test the album viewer's door hard. Get an
   independent security review. Write up how to
   [run your own server](sharing-and-names.md#your-own-server).

Throughout: family connections start on the relay and move to a direct path
when one works. Guests use the relay only as a last resort. The box's sign-in
page and apps are never on the internet. Guests never need an app or account.

## Where the code goes

Some of this exists; most is still to come.

```
crates/
  keys/           device and box keys, checking pinned keys, pairing
  link/           the iroh connection (our relays only), the local proxy, TCP 443 for forwarded ports
  punch/          signed address records, finding the box at home; cleverer hole punching later, if needed
  portmap/        opening the guests' door on the router (UPnP, NAT-PMP, PCP), IPv6 firewall rules
  proto/          messages between the box, the apps and the control plane
  ffi/            the bridge to the phone app's Swift and Kotlin code
desktop/          the desktop app: tray, start at sign-in, browser setup, installers
guest-viewer/     the guest page and its WebRTC code
connectd/         the box's side: port 443, device checks, share links, WebRTC for guests, address records
control-plane/    address records (iroh-dns-server), DNS for box names, certificates, the outside check, guests' setup messages
relay/            iroh-relay set up for ZOOMRIYA: who it lets in, limits, numbers; the guests' relay (TURN)
sim/              the 1,000-family test: traffic model, network lab, two-month soak
docs/             these pages and the diagrams
```
