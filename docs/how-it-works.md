# How it works

Part of [ZOOMRIYA Connect](../README.md).

This page describes the connection from the first attempt to the secure link.
Connect prefers a direct connection between a paired device and the box. A
small control plane helps them find each other. If direct paths fail, an
optional relay forwards the encrypted traffic.

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
- **Relay use is explicit.** Direct paths are preferred. If they fail, the
  endpoint may use only the configured relay operated by ZOOMRIYA or the
  family; the design does not silently fall back to unrelated public relays.
  The relay forwards encrypted packets and cannot authenticate a new device
  or read application content. A packet-capture test checks the allowed
  endpoints whenever iroh is upgraded.

## Guest links

- **A temporary route, only while a link is active.** While at least one
  guest link is live, the box asks the router to open port 443 using UPnP,
  NAT-PMP or PCP, or to allow access over IPv6. The route leads to a separate
  read-only share listener. It does not expose the sign-in page or apps. When
  no guest links are live, the box closes the route.
- **If a direct route is not available,** the guest's browser and the box
  try to connect using WebRTC. This also uses hole punching. If the network
  will not allow it, they may use the configured WebRTC relay. Only the
  encrypted, token-scoped share session is relayed.
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
authenticated direct path wins. If all direct attempts fail, the configured
relay provides a fallback. A relay increases latency and bandwidth cost and
reveals connection metadata to its operator, but end-to-end encryption keeps
application content private.

## Box name and control plane

Each box has a name, such as `maplehouse.zoomriya.app`, that points at the
box's address on the home network, never at the home's public address. A
family's own domain can point at it with a CNAME. Away from home, the app
finds the box through the control plane's signed, short-lived address
records, STUN and signaling, not through DNS.

The box has a real HTTPS certificate issued by Let's Encrypt, and its private
key stays on the box. To verify that the box controls its name, the control
plane publishes a temporary DNS record for the certificate check; no inbound
port is needed. Each box can change only its own records. Reaching the box
through a third-party dynamic DNS name, without ZOOMRIYA Connect, is not
supported; see
[Sharing, names and certificates](sharing-and-names.md#third-party-dynamic-dns).

## What carries data

The control plane helps devices find each other and is not the application
data path. Family traffic normally travels directly between a paired device
and the box over the encrypted link. If direct paths fail, a configured
relay forwards the encrypted stream between them. Guest album bytes normally
travel directly between the browser and the box's isolated share listener,
over HTTPS or a WebRTC data channel. If direct guest paths fail and relay
sharing is enabled, a relay forwards the encrypted share session only. The
guest viewer page itself may be downloaded from `share.zoomriya.net`; it is
separate from album data. DNS carries only DNS records and queries.

![Diagram showing DNS and connection setup separately from direct encrypted family and guest data. An optional relay carries encrypted traffic only after direct connection fails.](data-flow.svg)

![The web view's own HTTPS to the box runs inside the link, between the app's local proxy and connectd, so neither of them can open it.](link.svg)

### Every path, in order

![Family devices try six direct paths, then use an optional encrypted relay if all fail. Guests use direct HTTPS or WebRTC hole punching first and an optional relay fallback for the share session.](paths.svg)

## The parts of the system

| Part | Runs on | Job |
|---|---|---|
| **The link** | An iroh endpoint inside the ZOOMRIYA phone app (Android, iOS) and desktop app (Linux, Windows, macOS) | Connects to the box using its pinned endpoint ID over an authenticated QUIC connection. The app's web view, or a laptop browser through a local proxy, uses the link to reach the box's apps. It needs no VPN permission and can coexist with a work VPN. |
| **connectd** | The ZOOMRIYA Box | Runs the box's iroh endpoint on UDP 443 and owns TCP 443. It verifies paired devices and guest-link tokens, passes approved connections to the box's web server, coordinates hole punching, checks IPv6 reachability and requests temporary router access for guest links. It also publishes the box's signed address record and watches public certificate logs for the box's name. |
| **Control plane** | A small service run by ZOOMRIYA or the family | Serves signed address records, provides STUN and signaling, answers DNS for box names, hosts certificate challenges and checks reachability. It coordinates but does not carry application data. |
| **Relay** | A service run by ZOOMRIYA or the family, when enabled | Forwards encrypted family streams or guest share sessions only after direct paths fail. It can observe endpoint IPs, timing and traffic volume, but cannot read end-to-end encrypted content. |
