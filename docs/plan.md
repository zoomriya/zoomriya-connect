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
3. **[Build the family link](how-it-works.md).** First verify that iroh can
   connect without a relay; if not, use `quinn` with our own hole punching.
   Then implement pairing, the link, signaling and the local proxy. Include
   the advanced hole-punching paths from the start, then bring the link to
   Android and iOS.
4. **[Build the desktop app](desktop.md).** Start with a Linux proxy for
   testing, then add a tray app, automatic startup and PAC configuration for
   Linux, Windows and macOS. Package and sign the installers.
5. **[Add guest sharing](sharing-and-names.md#sharing-with-guests).** Add
   share tokens and the direct connection first, then the browser viewer and
   WebRTC route, owner controls and a check that tells owners whether guests
   are likely to connect.
6. **Support advanced network setups.** Add manual port forwarding, IPv6
   firewall rules and the TCP 443 fallback.
7. **[Support custom domains](sharing-and-names.md#names-and-domains),**
   whether they use ZOOMRIYA's control plane or the family's own DNS.
8. **Measure and improve reliability.** Measure which connection method
   works and why others fail across ISPs and NAT types. Then focus
   improvements on the most common failures.
9. **Prepare for release.** Test the exposed share listener, arrange an
   independent security review and document how to run a private control
   plane.

The design does not include relaying family or guest traffic (see
[What Connect is designed to do](../README.md#what-connect-is-designed-to-do)).
The box's sign-in page and apps will not be exposed directly to the internet;
family members use the apps when away from home. Guests will not need to
install anything. If the home has no direct route for guests, their browser
will try hole punching.

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
