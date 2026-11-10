# How it works

Part of [ZOOMRIYA Connect](../README.md).

This page describes the connection from the first attempt to the secure link.
Each connection between a paired device and the box starts through a relay
run by ZOOMRIYA or the family, then moves to a direct path as soon as one
works, the way iroh does it. If no direct path works, the relay keeps
forwarding the encrypted traffic. A small control plane helps the two ends
find each other.

## Family devices

- **Relay first, then direct.** Phones, laptops and the box run an iroh
  endpoint. They use QUIC, a transport protocol, and authenticate each other
  by key rather than relying only on network addresses. Each endpoint keeps a
  connection to its home relay. A new connection starts through the relay, so
  it opens at once. Meanwhile the relay tells each side the public address it
  sees and passes the signals for hole punching, and both sides try direct
  paths. When one works, traffic moves to it and the relay drops out. If none
  works, the relay keeps forwarding the encrypted traffic.
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
- **Only our relays.** The endpoint uses only relays run by ZOOMRIYA or the
  family; iroh's public relays and address lookup are switched off
  (`RelayMode::Custom` with our relay map, and our own address lookup). The
  relay admits only boxes and their paired devices: for each endpoint ID it
  asks the control plane (iroh-relay's `access = http`). It forwards encrypted packets
  and cannot authenticate a new device or read application content. A
  packet-capture test checks the allowed endpoints whenever iroh is upgraded.
- **Backups wait for a direct path.** Browsing, chat and the rest work over
  the relay; photo and video backups wait until the connection is direct, so
  the relay never carries bulk uploads.

## Guest links

- **A temporary route, only while a link is active.** While at least one
  guest link is live, the box asks the router to open port 443 using UPnP,
  NAT-PMP or PCP, or to allow access over IPv6. The route leads to a separate
  read-only share listener. It does not expose the sign-in page or apps. When
  no guest links are live, the box closes the route.
- **If a direct route is not available,** the guest's browser and the box
  try to connect using WebRTC. This also uses hole punching. If the network
  will not allow it, they use a WebRTC relay as the last option. Only the
  encrypted, token-scoped share session is relayed.
- **IPv6 is checked first.** The box checks whether it has a public IPv6
  address and whether outside connections to port 443 can reach it. The
  control plane runs that outside check. If IPv6 is reachable, guests with
  IPv6 can connect directly, including when the provider uses carrier-grade
  NAT for IPv4.

## When a direct connection is difficult

Some routers change connection addresses and ports in ways that make
hole-punching harder. iroh tries the home network, IPv6 and ordinary hole
punching. Where measurements show they help, ZOOMRIYA Connect may add harder
attempts: multiple UDP sockets, port prediction for strict (symmetric) NAT
and TCP simultaneous open. When nothing direct works, the connection simply
stays on the relay: slower, and it costs relay bandwidth, but end-to-end
encryption keeps the content private.

## Box name and control plane

Each box has a name, such as `maplehouse.zoomriya.app`, that points at the
box's address on the home network, never at the home's public address. A
family's own domain can point at it with a CNAME. Away from home, the app
finds the box through its signed, short-lived address record, which names
the box's relay, not through its name. These are iroh's own address records
(signed by the endpoint's key and served by `iroh-dns-server`), run by the
control plane.

The box has a real HTTPS certificate issued by Let's Encrypt, and its private
key stays on the box. To verify that the box controls its name, the control
plane publishes a temporary DNS record for the certificate check; no inbound
port is needed. Each box can change only its own records. Reaching the box
through a third-party dynamic DNS name, without ZOOMRIYA Connect, is not
supported; see
[Sharing, names and certificates](sharing-and-names.md#third-party-dynamic-dns).

## What carries data

The control plane helps devices find each other and is not the application
data path. Family traffic starts through the relay and moves to a direct
path between the paired device and the box when one works. If none works,
the relay keeps forwarding the encrypted stream between them. Guest album bytes normally
travel directly between the browser and the box's isolated share listener,
over HTTPS or a WebRTC data channel. If direct guest paths fail, a relay
forwards the encrypted share session only, as the last option. The
guest viewer page itself may be downloaded from `share.zoomriya.net`; it is
separate from album data. DNS carries only DNS records and queries.

![Diagram showing DNS and address records separately from encrypted family and guest data. Family connections start through the relay and move to a direct path when one works.](data-flow.svg)

![The web view's own HTTPS to the box runs inside the link, between the app's local proxy and connectd, so neither of them can open it.](link.svg)

### Every path, in order

![Family devices start each connection through the relay and try six direct paths at once; the first that works takes over, and the relay keeps carrying the connection only if none does. Guests use direct HTTPS or WebRTC hole punching first, with a relay as the last option for the share session.](paths.svg)

## The parts of the system

| Part | Runs on | Job |
|---|---|---|
| **The link** | An iroh endpoint inside the ZOOMRIYA phone app (Android, iOS) and desktop app (Linux, Windows, macOS) | Connects to the box using its pinned endpoint ID over an authenticated QUIC connection. The app's web view, or a laptop browser through a local proxy, uses the link to reach the box's apps. It needs no VPN permission and can coexist with a work VPN. |
| **connectd** | The ZOOMRIYA Box | Runs the box's iroh endpoint on UDP 443 and owns TCP 443. It verifies paired devices and guest-link tokens, passes approved connections to the box's web server, coordinates hole punching, checks IPv6 reachability and requests temporary router access for guest links. It also publishes the box's signed address record and watches public certificate logs for the box's name. |
| **Control plane** | A small service run by ZOOMRIYA or the family | Serves the boxes' signed address records (which name each box's relay), answers DNS for box names, hosts certificate challenges, checks reachability, and passes guests' WebRTC signals and STUN. It does not carry application data. |
| **Relay** | iroh's relay server, run by ZOOMRIYA or the family | Starts every family connection, tells each side its public address and passes the hole-punching signals; keeps forwarding encrypted traffic only when no direct path works. Also relays a guest's share session as the last option. Admits only boxes and their paired devices. It can observe endpoint IPs, timing and traffic volume, but cannot read end-to-end encrypted content. |
