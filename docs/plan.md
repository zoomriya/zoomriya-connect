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
| Run DNS for box names on the control plane: one server now | Simple to start |
| Ready for a second DNS server: changes go out as zone transfers, and `ns2.zoomriya.net` is reserved (pointing at the same server until then) | Turning a second on is a setting, not a rebuild. Most registrars ask for two name servers |
| Publish where the box is, and check it's on a cable | The box can't use Wi-Fi |

### 2. [Get ready for guests](where-it-works.md#guests)

| Task | Why, or what it fixes |
|---|---|
| Check whether the home has working IPv6 | Guests can come straight in, even behind a shared address |
| Open the door on the router automatically, to the box's port 8443, never 443 | Some routers make internet traffic look local. The port can't be faked |
| Check from outside that the door works and the sign-in page can't be reached through it | Catches a door that leaks, and closes it |
| When it doesn't work, explain why in plain words, with steps for common routers | Families aren't left guessing |
| Let owners switch guest access off | No door at all, if they prefer |

### 3. [Build the family link](how-it-works.md)

| Task | Why, or what it fixes |
|---|---|
| **Relays:** our own iroh relays, iroh's public ones off, address discovery on (`enable_quic_addr_discovery`) | Only boxes and paired devices use them |
| **Relay limits:** monthly volume and speed per box, slowed but never cut off, shown in Box Desktop | The relay can't tell backups from browsing, so it doesn't trust the app |
| **Registering boxes:** one name per box, blocked look-alike names, a daily limit per address, 90 days before a released name is free, reporting, a factory token for ready-made boxes | Anyone can run ZOOMRIYA OS and register |
| **Passes:** 30 days, naming the current link key, with a start and an end | Only paired devices get through the relay |
| **Link keys:** a new one with each pass, signed by the device key in the security chip. The box pins the device key | Most security chips can't hold iroh's Ed25519 key |
| **Renewal:** with under three weeks left, over any connection, including background work | Passes renew without anyone noticing |
| **Clock:** the box signs only when its clock is set from the internet. The relay allows 10 minutes' difference | A wrong clock would sign passes that have already run out |
| **Removed devices:** the box tells the relay at once, and on every reconnect | The relay refuses them too |
| **Control plane:** tells relays which boxes are registered, never which devices | It doesn't learn your devices |
| **Finding the box:** our own address server (`iroh-dns-server`). Apps keep the last signed record | Still works when it's down |
| **A family's own relay:** learns its box from a one-time code typed into Box Desktop | Families can run their own |
| **Device keys** in the security chip or key store. Backups off our relay | |
| **Web server:** connectd labels each connection with its device (PROXY protocol v2). ZOOMRIYA ID locks a person for 15 minutes after 5 wrong passwords | Connections from away would otherwise look like the box, and share one sign-in limit |
| **Phones:** iOS 17 or later, Android with a WebView that has proxy support. Older phones: home only, and the app says so | Older web views can't use the link's proxy |
| **iroh:** one exact version, no fork. First check whether the relay's access hook can see a pass; if not, the device hands it over first. Upgrade every quarter, with the traffic recording | iroh changes often |
| **Versions:** servers accept the current protocol version and the one before. New minimums announced months ahead | Old boxes and apps fail clearly |
| If the soak test shows iPhones missing renewals, make passes longer | Rather than waking the app more often |
| Cleverer hole punching, only if measurements show we need it | |

### 4. [Build the desktop app](desktop.md)

| Task | Why, or what it fixes |
|---|---|
| A plain Linux proxy for testing first | |
| The proxy only carries the box's own names | It can't be used to carry anything else |
| Tray icon, start at sign-in, browser setup on Linux, Windows and macOS | |
| Signed installers | |

### 5. [Add share links](sharing-and-names.md#sharing-with-guests)

| Task | Why, or what it fixes |
|---|---|
| Tokens, the guest page, the door, WebRTC, then the relay as the last resort | Guests need nothing installed |
| Owners' controls, and telling them what to expect | |
| The door's name, made by the box: `<name>-share.zoomriya.app`. Setup refuses names ending in `-share`, and `share` | No family's box can clash with another's door |
| On an own domain: `share.home.smith.ca`, covered by the NS record or its own two CNAMEs | |
| The box's web server uses `__Host-` cookies and checks each request's `Origin` | Without the Public Suffix List, one box could otherwise set cookies for another |
| **Guests' relay (TURN):** blocked private addresses, one-hour credentials per share, per-guest limits | It can't be used to reach private networks |

### 6. Help advanced homes

| Task | Why, or what it fixes |
|---|---|
| Manual port forwards and IPv6 firewall rules | |
| A direct path over TCP for families who forward a port: TLS with the same key checks, on the door's port 8443, never the web server's. Families type 8443; Box Desktop shows it | iroh's direct paths are UDP only |
| On only when the outside check finds the forward working; used only when UDP is blocked | Matters most to families who switch the relay off |

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

### 9. Get ready for release

| Task | Why, or what it fixes |
|---|---|
| Test the album viewer's door hard | It's the only part facing the internet |
| An independent security review | |
| Write up [running your own server](sharing-and-names.md#your-own-server) | |

### Always

| Rule |
|---|
| Family connections start on the relay and move to a direct path when one works |
| Guests use the relay only as a last resort, and never need an app or an account |
| The box's sign-in page and apps are never on the internet |

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
desktop/          the desktop app: tray, start at sign-in, browser setup, installers
guest-viewer/     the guest page and its WebRTC code
connectd/         the box's side: ports 443 and 8443, device checks, device labels for the web server, passes, share links, WebRTC for guests, address records
control-plane/    address records (iroh-dns-server), DNS for box names, certificates, the outside check, guests' setup messages
relay/            iroh-relay set up for ZOOMRIYA: checking passes and removed devices, limits, numbers; the guests' relay (TURN): blocked private addresses, one-hour credentials per share, per-guest limits
sim/              the 1,000-family test: traffic model, network lab, two-month soak
docs/             these pages and the diagrams
```
