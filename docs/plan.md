# Plan and layout

Part of [ZOOMRIYA Connect](../README.md).

This page is for the people building ZOOMRIYA Connect: what we build, in what
order, and where the code goes.

## What we build, in order

1. **[Give each box a name](sharing-and-names.md#names-and-domains).**
   - Build `connectd`, the box's side, on port 443.
   - Get the box a Let's Encrypt certificate through DNS, and run DNS for box
     names.
   - Publish where the box is, and check it's plugged in with a cable.
2. **[Get ready for guests](where-it-works.md#guests).**
   - Check whether the home has working IPv6.
   - Try opening the door on the router automatically, then test from outside
     whether it worked.
   - When it didn't, explain why in plain words, and show the steps for
     common routers.
   - Let owners switch guest access off.
3. **[Build the family link](how-it-works.md).**
   - **Relays.** Run our own iroh relays, with iroh's public ones switched
     off. They let in only registered boxes, and devices with a pass from one.
     Switch on address discovery (`enable_quic_addr_discovery`, off by
     default).
   - **[Passes](how-it-works.md#the-box-hands-out-passes).**
     - The box signs a 30-day pass for each paired device: the device's key,
       the box's key, and an end date.
     - It renews the pass when less than three weeks are left, over any
       connection to the box, including the app's background work (backups,
       refresh).
     - The relay checks passes itself, and lets each device reach only the box
       that signed its pass.
     - The box sends the relay its list of removed devices each time it
       connects, and again at once when one is removed.
     - The control plane tells relays which boxes are registered, never which
       devices.
     - If the soak test shows iPhones missing renewals, make passes longer
       rather than waking the app more often.
   - **Finding the box.** Run our own address server (`iroh-dns-server`). Apps
     keep the box's last signed address record, and use it when the address
     server can't be reached.
   - **A family's own relay** learns its box from a one-time code it shows at
     setup, typed into Box Desktop. A family's own `connect.` registers the
     box with its relay, as ours does.
   - **Pairing, the link and the desktop proxy.** Keep each device's key
     locked by its security chip or key store. Keep backups off our relay.
   - Bring it all to Android and iOS.
   - Add cleverer hole punching only if measurements show we need it.
4. **[Build the desktop app](desktop.md).** Start with a plain Linux proxy for
   testing. Then add the tray icon, starting at sign-in, and browser setup for
   Linux, Windows and macOS. Sign the installers.
5. **[Add share links](sharing-and-names.md#sharing-with-guests).** Tokens,
   the guest page, direct connections and WebRTC, with the relay as the last
   resort. Give owners their controls, and tell them what to expect.
   - The guests' door gets its own name, made by the box. Families never see
     it.
     - On ZOOMRIYA names: `<name>-share.zoomriya.app`. Setup refuses box names
       ending in `-share`, and the name `share`, so no family's box can clash
       with another's door.
     - On a family's own domain: `share.<box name>`, such as
       `share.home.smith.ca`, so the one NS record they already added covers
       it.
   - Before the first box ships, put `zoomriya.app` on the Public Suffix List,
     so browsers treat every family's names as separate sites. One family's
     box must never be able to set cookies for another's. Browsers take a
     while to pick up the list.
6. **Help advanced homes.**
   - Manual port forwards and IPv6 firewall rules.
   - For families who forward port 443: a direct path over TCP, for networks
     that block UDP. iroh doesn't do this, so it's our own: TLS with the same
     key checks, sharing TCP 443 with the guests' album viewer. The TLS
     handshake says which one each connection is for.
   - The box turns this on only when its outside check finds the forward
     working, and the app uses it only when UDP is blocked. It matters most to
     families who switch the relay off.
7. **[Own domains](sharing-and-names.md#names-and-domains).** A CNAME from the
   family's domain to their ZOOMRIYA name. Dynamic DNS from other companies
   isn't supported.
8. **[Test at scale](testing.md).** Simulate 1,000 families for two months:
   how many connections go direct, how fast they open, what the relay carries
   and costs, and what breaks. Then fix the most common failures first.
9. **Get ready for release.** Test the album viewer's door hard. Get an
   independent security review. Write up how to
   [run your own server](sharing-and-names.md#your-own-server).

These hold throughout:

- Family connections start on the relay and move to a direct path when one
  works.
- Guests use the relay only as a last resort, and never need an app or an
  account.
- The box's sign-in page and apps are never on the internet.

## Where the code goes

Some of this exists; most is still to come.

```
crates/
  keys/           device and box keys, checking pinned keys, pairing, making and checking passes
  link/           the iroh connection (our relays only), the local proxy, TCP 443 for forwarded ports
  punch/          signed address records, finding the box at home; cleverer hole punching later, if needed
  portmap/        opening the guests' door on the router (UPnP, NAT-PMP, PCP): TCP 443 for guests, UDP 443 for paired devices; IPv6 firewall rules
  proto/          messages between the box, the apps and the control plane
  ffi/            the bridge to the phone app's Swift and Kotlin code
desktop/          the desktop app: tray, start at sign-in, browser setup, installers
guest-viewer/     the guest page and its WebRTC code
connectd/         the box's side: port 443, device checks, passes, share links, WebRTC for guests, address records
control-plane/    address records (iroh-dns-server), DNS for box names, certificates, the outside check, guests' setup messages
relay/            iroh-relay set up for ZOOMRIYA: checking passes and removed devices, limits, numbers; the guests' relay (TURN)
sim/              the 1,000-family test: traffic model, network lab, two-month soak
docs/             these pages and the diagrams
```
