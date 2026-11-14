# Plan and layout

Part of [ZOOMRIYA Connect](../README.md).

This page is for the people building ZOOMRIYA Connect: what we build, in what
order, and where the code goes.

## What we build, in order

### 1. [Give each box a name](sharing-and-names.md#names-and-domains)

| Task | Why, or what it fixes |
|---|---|
| Build `connectd`, the box's side | Everything else hangs off it |
| One certificate per box, through DNS: Let's Encrypt first, Google Trust Services as the fallback. Ask Let's Encrypt for a higher limit before launch | `zoomriya.app` isn't on the Public Suffix List, so Let's Encrypt allows 50 new certificates a week across all boxes |
| Run DNS for box names on the control plane (`ns1.zoomriya.net`) | Simple to start |
| **A second DNS server for `ns2.zoomriya.net`, planned for release,** at another hosting company. It copies the first through zone transfers. Until it's up, `ns2.` points at the same server | Names keep answering if one server or hosting company is down. Most registrars ask for two name servers |
| Publish where the box is, and check it's on a cable | The box can't use Wi-Fi |

### 2. [Get ready for guests](where-it-works.md#guests)

| Task | Why, or what it fixes |
|---|---|
| Check whether the home has working IPv6, and whether the router will open an IPv6 door on a second address | Guests can come straight in, even behind a shared address. A bonus: when it can't, guests fall back, and setup says what to expect |
| Open the door on the router automatically, to the box's port 8443, never 443 | Some routers make internet traffic look local. The port can't be faked |
| Check from outside that the door works and the sign-in page can't be reached through it | Catches a door that leaks, and closes it |
| When it doesn't work, explain why in plain words, with steps for common routers | Families aren't left guessing |
| Let owners switch guest access off | No door at all, if they prefer |

### 3. [Build the family link](how-it-works.md)

| Task | Why, or what it fixes |
|---|---|
| **Relays:** our own iroh relays, iroh's public ones off, QUIC address discovery on (`QuicConfig` in the `iroh-relay` library) | Only boxes and paired devices use them |
| **Relay limits:** monthly volume and speed per box, slowed but never cut off, shown in Box Desktop | The relay can't tell backups from browsing, so it doesn't trust the app |
| **Registering boxes:** one name per box, blocked look-alike names, at most 10 a day per address (factory-token boxes don't count), 90 days before a released name is free, reporting, a factory token for ready-made boxes (one per box, one registration only) | Anyone can run ZOOMRIYA OS and register |
| **Passes:** 30 days, naming the current link key, with a start and an end | Only paired devices get through the relay |
| **Link keys:** a new one with each pass, signed by the device key in the security chip. The box pins the device key | Most security chips can't hold iroh's Ed25519 key |
| **Renewal:** with under three weeks left, over any connection, including background work | Passes renew without anyone noticing |
| **Clock:** the box signs only when its clock is set from the internet. The relay allows 10 minutes' difference. If the clock can't be set, Box Desktop's bell says so at once | A wrong clock would sign passes that have already run out |
| **Removed devices:** the box tells the relay at once, and on every reconnect | The relay refuses them too |
| **Control plane:** tells relays which boxes are registered, never which devices | It doesn't learn your devices |
| **Admin notes:** the box signs one for each admin's phone. The control plane accepts a name move or "stolen" only with one | Only admins can, and the control plane never keeps a device list |
| **Finding the box:** our own address server (`iroh-dns-server`). The record names only the box's relay, as iroh does by default, so the home's internet address is never published. With the relay switched off, the record holds the box's addresses instead, and Box Desktop says so. Apps keep the last signed record, and the box's own addresses, which the box sends them over the connection | Still works when it's down |
| **A family's own relay:** learns its box from a one-time code typed into Box Desktop | Families can run their own |
| **Device keys** in the security chip or key store, usable after the first unlock since the phone started (iOS's "after first unlock"). Backups off our relay | Passes renew in the background while the phone is locked |
| **Web server:** connectd labels each connection with its device (PROXY protocol v2), on a local socket only connectd can use; TCP 443 never accepts labels. ZOOMRIYA ID locks a person for 15 minutes after 5 wrong passwords | Connections from away would otherwise look like the box, and share one sign-in limit |
| **Phones:** iOS 17 or later, Android with a WebView that has proxy support. Older phones: home only, and the app says so | Older web views can't use the link's proxy |
| **iroh:** one exact version, 1.0 or later, never changed. The app sends its pass as the relay's token (`RelayMap::with_auth_token`). Our relay is built on the `iroh-relay` library with our own access check (`AccessControl`), which sees the device's proven key and its pass, and drops a removed device's live connections (`Clients::disconnect`). Upgrade every quarter, with the traffic recording | No fork to carry across each upgrade, and iroh changes often |
| **Connecting:** dial the relay and the box's addresses the app already knows (found on the home network, or remembered from the last connection) at the same moment. The first to answer carries data; hole punching goes through the relay; a direct path takes over when one works | Direct from the start at home and on IPv6, no wait behind difficult routers, stale addresses just lose |
| **Connection numbers:** iroh chooses the path; the link reads its measurements (time to connect and to direct, each path's round-trip time, time and data on the relay, path switches, hole punching), and failed and dropped connections with their reasons, and sends the box a summary after each connection. Failures are sent the next time it connects. Also counted: passes that didn't renew, address-record failures, switching to ZOOMRIYA's relay, backups waiting, time slowed by the relay limit, and guest links that opened or failed, by way. Box Desktop shows each device's history. Anonymous reports to us only if the family agrees, never naming a box or device | Families see why a device is slow, without us learning their devices |
| **Versions:** servers accept the current protocol version and the one before. New minimums announced months ahead | Old boxes and apps fail clearly |
| If the soak test shows iPhones missing renewals, make passes longer | Rather than waking the app more often |
| Cleverer hole punching, only if measurements show we need it | |
| Later: an admin's phone approves a new phone from away | Today pairing is only at home, so someone abroad who loses their phone waits until they're back |

### 4. [Build the browser extension for laptops](laptops.md)

| Task | Why, or what it fixes |
|---|---|
| **Check 1 first:** Box Desktop and one app running from the extension's address, fetched through a test connection: sign-in, `__Host-` cookies, `Origin` checks, WebSockets through an adapter | Everything else depends on it. If it fails, build the desktop app instead (below) |
| **Check 2:** the extension reaches our relay with its pass, and sets up WebRTC through it. In Chrome, WebRTC in a hidden extension page | Extensions can't use iroh's direct paths |
| **Check 3:** in the network lab, WebRTC's direct rate next to iroh's | Laptops shouldn't sit on the relay much more than phones |
| The extension's device key, which the browser won't export, signing its link key. Pairing at home with a short code that approves a key swap. Removal as for phones | The box pins it, as it pins phones |
| The box's side of WebRTC for paired laptops, next to the guests' | The box already speaks WebRTC to guests |
| Backups from laptops wait for a direct path, as phones' do | Big uploads stay off our relay |
| Chrome, Edge and Firefox, signed through their add-on stores | A copy from anywhere else can't update itself in |
| **Only if check 1 fails:** a desktop app sharing the phone app's link, with a local proxy that carries only the box's names, browser setup, and signed installers | Laptops still work |

### 5. [Add share links](sharing-and-names.md#sharing-with-guests)

| Task | Why, or what it fixes |
|---|---|
| Tokens, the guest page, the door, WebRTC, then the relay as the last resort | Guests need nothing installed |
| Owners' controls, and telling them what to expect | |
| The door's name, made by the box: `<name>-share.zoomriya.app`. Setup refuses names ending in `-share`, and `share` | No family's box can clash with another's door |
| On an own domain: `share.home.smith.ca`, covered by the NS record or its own two CNAMEs | |
| The box's web server uses `__Host-` cookies and checks each request's `Origin` | Without the Public Suffix List, one box could otherwise set cookies for another |
| **Guests' relay (TURN):** blocked private addresses, per-guest limits, and one-hour credentials that the box asks for after checking the token, and passes on in the WebRTC setup messages | It can't be used to reach private networks |

### 6. Help advanced homes

| Task | Why, or what it fixes |
|---|---|
| Manual port forwards and IPv6 firewall rules | |
| A direct path over TCP for families who forward a port: TLS with the same key checks, on the door's port 8443, never the web server's. Families forward the router's UDP 443 to the box's UDP 443, and TCP 443 to the box's 8443; Box Desktop shows both | iroh's direct paths are UDP only |
| On only when the outside check finds the forward working. The TCP path is tried only when UDP gets no answer | Matters most to families who switch the relay off |

### 7. [Own domains](sharing-and-names.md#your-own-domain-through-zoomriya)

| Task | Why, or what it fixes |
|---|---|
| One NS record, or four CNAMEs where the provider doesn't allow NS | One CNAME doesn't cover the certificate check or the door |
| Box Desktop shows the lines to copy and checks them | |
| No dynamic DNS from other companies | It would put the sign-in page on the internet |

### 8. [Test at scale](testing.md)

| Task | Why, or what it fixes |
|---|---|
| Simulate 1,000 families for two months: direct paths, opening speed, relay traffic and cost, failures | Real numbers before release |
| Set the relay's per-box limits from it, then fix the most common failures | |
| [Benchmarks](testing.md#benchmarks-on-every-change) on every change to `main`, like iroh's: time to connect and to direct, round-trip time, time to first byte, speed, reconnecting, failed and dropped connections, relay load. Compared with plain HTTPS and iroh alone. Fails when 10% worse. Results on a public page | Slowdowns caught before release |
| After release: probes in several regions measure our relays, a test box and DNS every 5 minutes | We hear about a slow relay before families do |

### 9. Get ready for release

| Task | Why, or what it fixes |
|---|---|
| Test the album viewer's door hard | It's the only part facing the internet |
| An independent security review, the browser extension included | |
| The second DNS server for `ns2.zoomriya.net` running, at another hosting company | Box names don't depend on one server |
| Write up [running your own server](sharing-and-names.md#your-own-server) | |
| [Running the service](operations.md): hourly backups to another company and a monthly restore test, alerts, an on-call rota of at least two, the status page, DNS protected from floods, a schedule for changing our keys | We find out first, and can rebuild |
| Boxes register again by themselves if their registration is missing, proving the name with their certificate's key, and take a new DNS update key when told | A restore that lost the last hour, or a leaked key, needs nothing from families |
| Decide how long each server keeps what it sees, and put it on the privacy page | [What our servers keep](operations.md#what-our-servers-keep) |

### 10. [Own Cloudflare Tunnel](sharing-and-names.md#your-own-cloudflare-tunnel)

| Task | Why, or what it fixes |
|---|---|
| Write the threat model and the protocol for the encrypted connection through Cloudflare: replays, retries, many requests at once, box restarts, switching option. Pick the cryptography only after it's reviewed | It's our own encryption, next to iroh's |
| The app's link and the extension: the same encrypted connection to the box, carried through the tunnel. Same pairing, keys and screens | One app and one extension for every option |
| **Switching option:** every device pairs again, at home, or with a pairing code an admin makes in Box Desktop and reads out, for example over the phone. The code works from away, once, for a limited time, only for a device already on the box's list | A switch can't let a new device in from away |
| **Backups through the tunnel only for files up to 90 MB,** leaving room under Cloudflare's 100 MB for our encryption. Bigger files wait until the device is home, then go over the home network. The app and the extension show which files are waiting | Free plans take at most 100 MB per upload. See [Cloudflare's limits](sharing-and-names.md#cloudflares-limits) |
| Ask Cloudflare whether tunnel traffic counts under the CDN terms on photos and video: backups, the family's playback, and guest albums | Their pages don't say |
| Pairing stays at home only, as in step 4 | Nobody pairs through Cloudflare |
| connectd's family door: checks the device's key, refuses replays, opens and seals, hands on to the web server with the device's label | The box alone decides who gets in |
| cloudflared on the box, set up from Box Desktop: `home.` to the family door, `share.` to the album viewer, nothing else. A tunnel key limited to the box's tunnel and names | The sign-in page and apps stay off the internet |
| Check what Cloudflare's own account flow allows before promising setup in one click | |
| Box Desktop says what Cloudflare can see before it's switched on, and when a link is made | Families choose knowing the trade-off |
| Test pairing, removal, switching option, extension updates, a lost tunnel, large photos and videos, and that `share.` reaches only the album viewer | |
| An independent security review of the protocol through Cloudflare | |

### Always

| Rule |
|---|
| Family connections try the box's addresses and the relay at once, and move to a direct path when one works |
| With ZOOMRIYA Connect, guests use the relay only as a last resort. Guests never need an app or an account |
| The box's sign-in page and apps are never on the internet, in any option |
| Every device is paired at home, in any option |

## Where the code goes

Some of this exists; most is still to come.

```
crates/
  keys/           device keys in the security chip, link keys signed by them, the box key with signatures that say what they're for, pinning, pairing, passes
  link/           the iroh connection (our relays only), the local proxy (box names only), TCP on the door's port for forwarded ports
  punch/          signed address records, finding the box at home; cleverer hole punching later, if needed
  portmap/        opening the guests' door on the router (UPnP, NAT-PMP, PCP): the router's TCP 443 to the box's 8443 for guests, UDP 443 for paired devices; over IPv6, a firewall rule for the box's own door address
  proto/          messages between the box, the apps and the control plane
  ffi/            the bridge to the phone app's Swift and Kotlin code
guest-viewer/     the guest page and its WebRTC code
extension/        the browser extension for laptops, every option: its key, pairing, the relay and WebRTC, the connection through Cloudflare, showing the box's pages
connectd/         the box's side: UDP 443 and TCP 8443 (the gateway keeps TCP 443), device checks, device labels for the web server, passes, share links, WebRTC for guests, address records; for option 4, the family door and setting up cloudflared
control-plane/    address records (iroh-dns-server), DNS for box names, certificates, the outside check, guests' setup messages
relay/            our relay, built on the iroh-relay library: our access check for passes and removed devices, limits, numbers; the guests' relay (TURN): blocked private addresses, one-hour credentials per share, per-guest limits
sim/              the 1,000-family test: traffic model, network lab, two-month soak; the benchmarks and probes
docs/             these pages and the diagrams
```
