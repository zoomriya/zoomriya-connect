# Where it works

Part of [ZOOMRIYA Connect](../README.md).

**The box must connect to the router by Ethernet, not Wi-Fi.** A wired
connection gives the box a more dependable place on the home network and
avoids Wi-Fi client-isolation rules that can block connections. It also helps
with port mapping and hole punching. Setup checks that the box is wired and
that its Ethernet port is on the same home network as the Wi-Fi devices.
Some provider gateways and mesh systems put TV or guest ports on a separate
network.

## A few network terms

The type of internet connection at home affects which connection paths are
available. These terms help explain the table below.

- **Wired home internet** (fibre, cable or DSL) brings a line into the house
  and connects it to a modem or gateway. Many homes have a public IPv4
  address, which can let the router forward port 443 to the box.
- **Home wireless internet** (5G, LTE or fixed wireless) receives the
  provider's signal at home and provides Wi-Fi and Ethernet, much like a
  phone hotspot. These providers may share a public IPv4 address among many
  customers.
- **CGNAT** (carrier-grade network address translation) is when the internet
  provider shares one public address among customers. In that case, the home
  router cannot make a port forward reachable from the internet. Ethernet
  does not remove this provider-side restriction.
- **Symmetric NAT** is a stricter form of address translation that may assign
  a different public port for each destination. It can stop ordinary hole
  punching, though Connect's more advanced attempts may work in some cases.
  Whether a particular provider uses it varies.
- **An open path** means the box can be reached from the internet on port 443,
  through router mapping, a port forward or an IPv6 firewall rule.
- **Hole punching** is a way for two devices behind routers to try to connect
  directly by coordinating outbound connection attempts. It can fail when
  either network restricts those connections.
- **UDP** is a network transport used by the direct connection and WebRTC.
  Some workplaces, hotels and other networks block it.
- **WebRTC** is a browser technology used here for a guest's direct connection
  to the box.
- **Easy NAT** in the table means the router's address translation allows the
  coordinated connection attempts to work. A **symmetric NAT** assigns
  destination-specific ports, which makes those attempts harder.

## By the home's setup

This table describes the intended behavior; these network combinations have
not yet been tested. "App" means the phone or desktop app. "Guest link" means
a guest opening a shared album in a browser; guest access is read-only.
**Yes** means the connection is expected to work; **Usually** means it
depends on the device and provider's IPv6 support.

| Home setup | Home Wi-Fi | App · easy NAT | App · symmetric NAT | App · UDP blocked | Guest link |
|---|---|---|---|---|---|
| **Regular** (default: nothing set up by hand, hole punching) | | | | | |
| Home router with easy NAT | Yes | Yes | Yes | No | Yes |
| Home router with symmetric NAT | Yes | Yes | Relay if enabled and reachable | Relay if enabled and reachable | Yes if the box could open 443; otherwise WebRTC, then relay if enabled and reachable |
| ISP's CGNAT, easy | Yes | Yes | Yes | Relay if enabled and reachable | Yes, by WebRTC, then relay if enabled and reachable |
| ISP's CGNAT, symmetric | Yes | Yes | Relay if enabled and reachable | Relay if enabled and reachable | WebRTC may fail; relay if enabled and reachable |
| Home and device both have IPv6 | Yes | Usually | Usually | Relay if enabled and reachable | Yes, for guests with IPv6; otherwise relay if enabled and reachable |
| **Advanced** | | | | | |
| Port forward, public IPv4 | Yes | Yes | Yes | Yes, TCP 443 | Yes |
| Double NAT, forwarded on both routers | Yes | Yes | Yes | Yes, TCP 443 | Yes |
| IPv6 firewall rule to the box | Yes | Yes | Yes | If the device has IPv6 | If the guest has IPv6 |
| ISP's CGNAT, no IPv6 | A forward can't help: direct access needs a public IPv4 or IPv6. Connect relay fallback can still work if enabled and reachable. | | | | |
| **Box on Wi-Fi** | Not supported: connect the box by Ethernet | | | | |
| **Home only** (remote access off) | Yes | No | No | No | No |

- **At home:** a browser can use the box normally. **Away from home:** a
  browser can open guest share links, but family members need the phone or
  desktop app for other box features.
- **Guest links** connect directly when the box can open port 443 on the
  router, through automatic mapping or an IPv6 rule, and only while a share
  link is active. Otherwise, the browser and box try WebRTC hole punching.
  If that fails, an enabled relay can carry the encrypted, read-only share
  session. The relay must be reachable and available.
- **Background backups away from home:** Android is designed to back up in
  the background using the link. iOS background uploads are handed to the
  system, which cannot use the link directly, so uploads may wait until the
  app is open or the phone is back on home Wi-Fi. iOS may grant short
  background windows that allow an upload; this still needs testing.
- **Port forwarding** helps only if the home router has a public IPv4 address.
  It cannot bypass the provider's CGNAT. Setup is designed to use an outside
  connectivity check to explain which case applies and show router-specific
  steps when a port forward can help.

## By the guest's network

Guests do not need to accept incoming connections. A direct link to a box
with an open path can therefore work from cellular data, home Wi-Fi, hotels
or offices. The guest's network matters more when the box has no open path
or only an IPv6 path:

- **Direct link to an open-path box:** works from cellular data too. Cellular CGNAT affects only connections coming *in* to the guest, not this one going out.
- **IPv6-only box path:** the guest's network must have IPv6, and many hotels and offices don't.
- **Browser hole-punch route** (box with no open path): works if at least one end is not symmetric NAT. A guest on cellular is behind CGNAT too, so the chance of both ends being strict is higher than for a guest on home Wi-Fi.
- **Relay fallback:** can work when direct paths fail, provided relay use is
  enabled and the guest network permits reaching the relay. The relay
  forwards encrypted traffic and adds latency and bandwidth cost.

## Never works

Some networks cannot use Connect: networks that allow traffic only to
approved sites, HTTPS-inspecting proxies (the pinned keys deliberately
reject them), and captive portals before sign-in. Hole punching also usually
fails when both ends use symmetric NAT, though the more advanced attempts
may help in some cases. If relay fallback is disabled, blocked or
unavailable, those direct-path failures still prevent remote access.

Family members on these networks need an allowed relay path or an open path
to the box for the apps' TCP 443 fallback. Guests may not be able to open a
shared link if both direct and relay paths are blocked; the owner is warned
about this when creating it.
