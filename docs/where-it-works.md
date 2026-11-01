# Where it works

Part of [ZOOMRIYA Connect](../README.md).

**Requirement:** the box must be connected to the router by Ethernet, not Wi-Fi. A wired box keeps a steady address for port forwards, gets reliable mappings, is not blocked by Wi-Fi client isolation, and hole punches more reliably. The setup checks the link type and tells the admin if the box is on Wi-Fi, and checks that the port is on the same network as the home Wi-Fi: some provider gateways and mesh routers have TV or guest ports on a network of their own.

## Terms: home internet types

Which kind of home internet the box sits on decides what can reach it.

- **Wired home internet** (fibre, cable, DSL): a line comes into the house and ends at a modem or gateway. Most of these homes get a public IPv4 address, so the router can be told to send port 443 to the box. This is the easy case.
- **Home wireless internet** (5G or LTE home internet, fixed wireless): there is no line into the house. A box in the home picks up the provider's **cell or radio signal** and gives the home Wi-Fi and Ethernet ports, much like a phone hotspot. Phone-style networks run short of public IPv4 addresses, so many customers share one address.
- **CGNAT (carrier-grade NAT):** the provider's equipment, not the home router, shares one public address among many customers. The home router can't tell it to forward port 443, and some providers say outright that port forwarding isn't allowed. Wiring the box by Ethernet doesn't change this, because the block is at the provider's end.
- **Symmetric NAT:** a strict kind of address translation that picks a new public port for every destination. Plain hole punching fails when both ends are behind it; harder hole punching (many sockets, port prediction) sometimes still succeeds. CGNAT on cell networks is more likely to be strict, though this varies by provider and is not confirmed.
- **Open path:** the box is reachable from the internet on port 443, through automatic mapping, a port forward or an IPv6 pinhole.

## By the home's setup

Based on how the pieces are designed. Nothing here is tested yet. "App" means the phone app or the desktop app; "Guest link" means a guest opening a shared album in a browser, read-only.

| Home setup | Home Wi-Fi | App · easy NAT | App · symmetric NAT | App · UDP blocked | Guest link |
|---|---|---|---|---|---|
| **Regular** (default: nothing set up by hand, hole punching) | | | | | |
| Home router with easy NAT | Yes | Yes | Yes | No | Yes |
| Home router with symmetric NAT | Yes | Yes | Harder punching only | No | Yes if the box could open 443; otherwise only from easy networks |
| ISP's CGNAT, easy | Yes | Yes | Yes | No | Yes, by WebRTC, from networks that allow UDP |
| ISP's CGNAT, symmetric | Yes | Yes | Harder punching only | No | Only from easy networks, by WebRTC |
| Home and device both have IPv6 | Yes | Usually | Usually | No | Yes, for guests with IPv6 |
| **Advanced** | | | | | |
| Port forward, public IPv4 | Yes | Yes | Yes | Yes, TCP 443 | Yes |
| Double NAT, forwarded on both routers | Yes | Yes | Yes | Yes, TCP 443 | Yes |
| IPv6 firewall rule to the box | Yes | Yes | Yes | If the device has IPv6 | If the guest has IPv6 |
| ISP's CGNAT, no IPv6 | A forward can't help: same as the regular CGNAT rows. The fix is a public IPv4 or IPv6 from the ISP | | | | |
| **Box on Wi-Fi** | Not supported: connect the box by Ethernet | | | | |
| **Home only** (remote access off) | Yes | No | No | No | No |

- **At home,** every browser has full access. **Away from home,** a plain browser gets share links only; everything else needs the phone app or the desktop app.
- **Guest links** go direct when the box could open 443 on the router (automatic mapping or an IPv6 pinhole, only while a share link is live). Otherwise they hole punch by WebRTC, which fails when both ends are strict or the guest's network blocks UDP.
- **Background backup away from home:** Android backs up in the background by default, because its backup uses the link. iPhones hand background uploads to the system, which can't use the link, so they wait until the app is open or the phone is back on home Wi-Fi. iOS background tasks, short windows the system picks, may let the app open the link and upload in between; to be tested.
- **A port forward,** automatic or manual, helps only when the home router has a public IPv4 address; it can't reach past an ISP's carrier-grade NAT. The setup tells the admin plainly which case their home is in, based on a test the control plane runs from outside, and shows the steps for their router when it can.

## By the guest's network

The guest only needs to send traffic out, so cellular data, home Wi-Fi, hotels and offices are all fine for a direct link to a box with an open path. The guest's network matters only when the box has no open path or only an IPv6 path:

- **Direct link to an open-path box:** works from cellular data too. Cellular CGNAT affects only connections coming *in* to the guest, not this one going out.
- **IPv6-only box path:** the guest's network must have IPv6, and many hotels and offices don't.
- **Browser hole-punch route** (box with no open path): works if at least one end is not symmetric NAT. A guest on cellular is behind CGNAT too, so the chance of both ends being strict is higher than for a guest on home Wi-Fi.

## Never works

Networks that allow only listed sites, proxies that open HTTPS (pinned keys refuse them on purpose), captive portals before sign-in, and, most of the time, symmetric NAT on both ends for hole-punched connections (harder hole punching rescues some of these, but don't count on it). Family members there need an open path at the box, for the apps' TCP 443 fallback. Guests there can't open the link, and the owner is told so when making it.
