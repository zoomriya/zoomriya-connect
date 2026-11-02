# Plan and layout

Part of [ZOOMRIYA Connect](../README.md).

## Plan

1. **[The box's own name](sharing-and-names.md#names-and-domains):** connectd on 443, a real certificate from Let's Encrypt through a DNS challenge, the control plane's DNS (A and AAAA) for box names, the box reporting its addresses, the Ethernet check.
2. **[Guest access without setup](where-it-works.md#by-the-homes-setup):** the IPv6 reachability check, automatic port mapping and IPv6 pinholes, the outside test with a specific reason for each failure, ISP and gateway detection with exact steps, and a turn-off switch.
3. **[The link and hole punching](how-it-works.md):** first a test that iroh hole punches well with no relay (if it can't, the link uses `quinn` and our own punching instead); then pairing keys, the link on iroh, our signaling, the local proxy; harder hole punching (many sockets, port prediction, parallel paths) from the start, not as an afterthought; Android, then iOS.
4. **[Desktop app](desktop.md):** a Linux proxy for testing, then the tray app with autostart and PAC for Linux, Windows and macOS, then signed installers.
5. **[Sharing](sharing-and-names.md#sharing-with-guests):** share tokens, the direct path, then the guest viewer with WebRTC hole punching, the owner's controls and the "will guests get in" check.
6. **Advanced setups:** manual port forwards and IPv6 rules, the TCP 443 fallback.
7. **[Custom domains](sharing-and-names.md#names-and-domains)**, through ZOOMRIYA or on their own.
8. **Measure and tune:** log which method worked and why others failed, by ISP and NAT type, from real homes, then fix the biggest failure group first.
9. **Hardening and release:** exposure tests (the share view with port 443 open to the internet), an independent security review, a guide to running your own control plane.

Not planned: a relay for family or guest traffic (see "No relay" in the README's [What we want](../README.md#what-we-want)); the box's sign-in page or apps on the internet (away from home, family members use the apps); anything for guests to install. Guests at a home with no open path use the browser hole-punch route.

## Layout

Planned; not all of it exists yet.

```
crates/
  keys/           endpoint and hardware keys, pinned-ID checks, pairing
  link/           the iroh endpoint (built-in public servers off, allow list), local CONNECT proxy, TLS-over-TCP fallback
  punch/          address lookup (signed records, mDNS), signaling, port prediction, many sockets
  portmap/        share-view port mapping (UPnP, NAT-PMP, PCP) and IPv6 pinholes
  proto/          messages shared with the control plane, versioned
  ffi/            bindings for the phone app's Swift and Kotlin code
desktop/          the desktop app: tray, autostart, PAC, installers
guest-viewer/     the static guest page and its WebRTC client
connectd/         the box side: port 443, device checks, share tokens, WebRTC for guests, hole punching, address reports
control-plane/    API, STUN, signed address records, signaling, DNS for box names, certificate challenges, outside reachability test
docs/             diagrams
```
