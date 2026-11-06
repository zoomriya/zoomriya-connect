# How it works

Part of [ZOOMRIYA Connect](../README.md).

This page describes the connection from the first attempt to the secure link.
In short, Connect tries to make a direct connection between a paired device
and the box. A small control plane helps them find each other, but does not
carry their traffic.

## Family devices

- **A direct connection is the goal.** Phones, laptops and the box run an
  iroh endpoint. They use QUIC, a transport protocol, and authenticate each
  other by key rather than relying only on network addresses. To connect
  through routers, the control plane tells each side the public addresses it
  has seen and coordinates their connection attempts. This technique is
  called hole punching.
- **No router changes are normally needed for paired devices.** The phone
  and desktop apps do not require an open port on the home router.
- **The connection is limited to the box.** It provides access to the box's
  apps, not to other devices on the home network. It is not a VPN.
- **Paired devices verify each other's keys.** Pairing starts at home: a
  phone scans a QR code displayed by the box. This lets the phone learn the
  box's endpoint ID (its public key), and the box learn the phone's. A
  hardware-backed key on the phone signs each connection. Only paired
  devices are allowed in, and an intermediary cannot pretend to be the box.
- **The box is quiet to unpaired devices.** A connection begins with a tag
  that can be made only by a device that knows the box's key. The box drops
  other first packets without replying, so a network scan cannot tell whether
  a Connect endpoint is there.
- **Only Connect's servers are used.** The design disables iroh's public
  relays, address lookup and automatic router mapping. An allow list limits
  each endpoint to the box and the configured control plane, whether run by
  ZOOMRIYA or by the family. A packet-capture test checks this whenever iroh
  is upgraded.

## Guest links

- **A temporary route, only while a link is active.** While at least one
  guest link is live, the box asks the router to open port 443 using UPnP,
  NAT-PMP or PCP, or to allow access over IPv6. The route leads to a separate
  read-only share listener. It does not expose the sign-in page or apps. When
  no guest links are live, the box closes the route.
- **If a direct route is not available,** the guest's browser and the box
  try to connect using WebRTC. This also uses hole punching. If the network
  will not allow it, the link cannot be opened.
- **IPv6 is checked first.** The box checks whether it has a public IPv6
  address and whether outside connections to port 443 can reach it. The
  control plane runs that outside check. If IPv6 is reachable, guests with
  IPv6 can connect directly, including when the provider uses carrier-grade
  NAT for IPv4.

## When a direct connection is difficult

Some routers change connection addresses and ports in ways that make
hole-punching harder. Connect is designed to try additional routes: it can
open multiple UDP sockets, predict ports used by strict (symmetric) NAT, and
try UDP, TCP simultaneous open and IPv6 paths at the same time. The first
authenticated path wins. This may help phones, desktop apps and guest
browsers, but it will not work on every network and does not use a relay.

## Box name and control plane

Each box has a name, such as `maplehouse.zoomriya.app`, and a real HTTPS
certificate issued by Let's Encrypt. The private key stays on the box. To
verify that the box controls its name, the control plane publishes a
temporary DNS record for the certificate check; an inbound port is not
needed for that check.

The control plane stores the box's signed address record, tells each device
the public address it has observed, coordinates connection attempts, answers
DNS requests for box names, hosts certificate challenges and checks from
outside whether the box is reachable. It stores no family data and cannot
approve a device. Families can run their own control plane.

![The web view's own HTTPS to the box runs inside the link, between the app's local proxy and connectd, so neither of them can open it.](link.svg)

### Every path, in order

![Family devices try six direct paths together: the home network, IPv6, hole punching, harder hole punching, the open path on UDP 443, and TLS over TCP 443; the first that authenticates wins. A relay, off for now, would come only after all six fail, never for backup or bulk uploads. Guests load a page from share.zoomriya.net, then reach the box directly over HTTPS through its share name, or by WebRTC hole punching; if neither works, the owner can send the photos instead. Guests never use a relay.](paths.svg)

## The parts of the system

| Part | Runs on | Job |
|---|---|---|
| **The link** | An iroh endpoint inside the ZOOMRIYA phone app (Android, iOS) and desktop app (Linux, Windows, macOS) | Connects to the box using its pinned endpoint ID over an authenticated QUIC connection. The app's web view, or a laptop browser through a local proxy, uses the link to reach the box's apps. It needs no VPN permission and can coexist with a work VPN. |
| **connectd** | The ZOOMRIYA Box | Runs the box's iroh endpoint on UDP 443 and owns TCP 443. It verifies paired devices and guest-link tokens, passes approved connections to the box's web server, coordinates hole punching, checks IPv6 reachability and requests temporary router access for guest links. It also publishes the box's signed address record and watches public certificate logs for the box's name. |
| **Control plane** | A small server run by ZOOMRIYA or the family | Serves the box's signed address record, helps each side discover its public address (STUN), coordinates connection attempts, passes guest signaling messages, answers DNS for box names, hosts certificate challenges and checks from outside whether the box can be reached over IPv4 and IPv6. It does not relay traffic. |
