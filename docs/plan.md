# Plan and layout

Part of [ZOOMRIYA Connect](../README.md).

## Plan

1. **[Give each box a name](sharing-and-names.md#names-and-domains).**
   Build `connectd` on port 443, issue a Let's Encrypt certificate through
   DNS validation, add control-plane DNS records, report the box's addresses
   and check that it uses Ethernet.
2. **[Support guest access](where-it-works.md#by-the-homes-setup).** Check
   whether IPv6 is reachable, try automatic router port mapping or IPv6
   access, and run an outside connectivity test. Explain failures in plain
   terms, identify common providers and gateways, show any relevant setup
   steps, and let owners turn guest access off.
3. **[Build the family link](how-it-works.md).** Run our own iroh relays
   (`iroh-relay`, iroh's public ones off) that admit only boxes and paired
   devices, and our own `iroh-dns-server` for the address records. Then
   implement pairing, the link and the local proxy, keep backups off the
   relay, and bring the link to Android and iOS. Add harder hole-punching
   attempts only where measurements show they help.
4. **[Build the desktop app](desktop.md).** Start with a Linux proxy for
   testing, then add a tray app, automatic startup and PAC configuration for
   Linux, Windows and macOS. Package and sign the installers.
5. **[Add guest sharing](sharing-and-names.md#sharing-with-guests).** Add
   scoped share tokens and the browser viewer. Try direct HTTPS and WebRTC
   first; add a WebRTC relay as the last option, carrying only the
   encrypted, read-only share session. Include owner controls and clear
   notice when a share may use relay bandwidth.
6. **Support advanced network setups.** Add manual port forwarding, IPv6
   firewall rules and the TCP 443 fallback.
7. **[Support custom domains](sharing-and-names.md#names-and-domains)**
   through a CNAME to the box's ZOOMRIYA name. Third-party dynamic DNS
   without ZOOMRIYA Connect is not supported.
8. **[Test at scale](testing.md).** Simulate 1,000 families over two
   months: how many connections go direct, how fast they open, what the
   relay carries and costs, and what breaks. Measure which connection method
   works and why others fail across ISPs and NAT types, then focus
   improvements on the most common failures.
9. **Prepare for release.** Test the exposed share listener, arrange an
   independent security review and document how to run
   [your own server](sharing-and-names.md#your-own-server).

Family connections start through the relay and move to a direct path when one
works; guests use a relay only as the last option. Relays forward end-to-end
encrypted traffic and are operated by ZOOMRIYA or the family. The box's sign-
in page and apps will not be exposed directly to the internet; family members
use the apps when away from home. Guests need no app or account and can use a
relay only for the scoped read-only share session.

## Layout

Planned; not all of it exists yet.

```
crates/
  keys/           endpoint and hardware keys, pinned-ID checks, pairing
  link/           the iroh endpoint (our relays only), local CONNECT proxy, TLS-over-TCP fallback
  punch/          signed address records, mDNS; later, harder hole punching if measurements call for it
  portmap/        share-view port mapping (UPnP, NAT-PMP, PCP) and IPv6 pinholes
  proto/          messages shared with the control plane, versioned
  ffi/            bindings for the phone app's Swift and Kotlin code
desktop/          the desktop app: tray, autostart, PAC, installers
guest-viewer/     the static guest page and its WebRTC client
connectd/         the box side: port 443, device checks, share tokens, WebRTC for guests, address reports
control-plane/    API, signed address records (iroh-dns-server), DNS for box names, certificate challenges, outside reachability test, guest signaling and STUN
relay/            iroh-relay set up for ZOOMRIYA: who it admits (access = http), rate limits, metrics; TURN for guests
sim/              the 1,000-family test: traffic model, network lab, two-month soak
docs/             diagrams
```
