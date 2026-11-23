# Direct+

Design from the discussion of 23 November 2026, built on the `direct+`
branch. On this branch the ZOOMRIYA Connect servers (control plane, relay,
their GUI), the box's code that talked to them, and the browser extension
are removed; `main` keeps them. The comparison with ZOOMRIYA Connect is in
[zoomriya-docs/compare.md](../../zoomriya-docs/compare.md).

## 1. Summary

| | |
|---|---|
| What | Reaching the box from away **without ZOOMRIYA Connect**: no ZOOMRIYA server, no VPS, no second box |
| How | The phone or laptop app finds the box through the BitTorrent DHT and connects directly with iroh. When no direct path works, it goes through Tor to **the box's own iroh relay**, keeps punching, and moves to the direct path without a drop |
| Who | The ZOOMRIYA apps on phones and laptops. Plain browsers work at home; guests through a door when the home can be reached |
| Principle | **Direct reach first, use Tor less second, make the Tor fallback faster third** |
| Goal for Tor | **Families never notice its delay** |
| Limit | Cannot promise access through every restrictive network |

### What works and what does not

| | Works | Does not work |
|---|---|---|
| At home | ✅ Everything: apps, Box Desktop, browsers, with or without internet | — |
| Phone and laptop apps, away, home can be reached (public IPv4 or open IPv6) | ✅ Full speed | — |
| Apps away, home on CGNAT or behind a router that refuses ports | ✅ Mostly full speed; the rest through Tor, which families do not notice | — |
| Apps away, network blocks Tor | ✅ When a direct path or Tor bridges work | ❌ Otherwise: use mobile data |
| Apps away, network allows only approved websites | — | ❌ Use mobile data |
| Browser at home, device without the app | ✅ `zoomriya-xxxx.local` after trusting the box once (in the background, see [§11](#11-names-and-certificates-for-browsers)), or the family's own domain or dynamic DNS with a real padlock | — |
| Browser on a laptop, away | ✅ Any browser, with the laptop app connecting in the background | ❌ Without the app |
| Guests, home can be reached | ✅ Through the door, with the family's own domain or dynamic DNS | — |
| Guests, home on CGNAT with IPv6 | ✅ Guests with IPv6 | ❌ Guests with IPv4 only |
| Guests, home on CGNAT without IPv6 | ✅ Onion link (Tor Browser), or send the photos | ❌ A normal link in any browser |
| Home internet down | ✅ Copies already on the device | ❌ Reaching the box |

## 2. Rules

| Rule | |
|---|---|
| No ZOOMRIYA relay, no VPS, no second box | Outside the home there are only volunteer networks: Tor and the DHT |
| No SIM in the box, and the box is not the home's router | Everything is software on the box, plugged into the family's router, and on the apps |
| Phones and laptops get native apps | Laptops: a ZOOMRIYA app (Tauri), the same Rust core as the phone. The browser extension is dropped |
| Tor is never named on screen | Only in Box Desktop's technical details |
| Optional for the OS's internet switches | **Apps offline** or **Everything offline** turns Direct+ off |

## 3. How a connection is made

| Step | What happens |
|---|---|
| 1 | The app tries the box's remembered addresses and its relay through Tor, and at the same time looks on the home network (mDNS) and up the box's DHT record. The first to connect wins |
| 2 | It dials every address it has, IPv6 and IPv4, Wi-Fi and mobile data together. The first to answer carries data |
| 3 | If nothing answers, it connects through Tor to the box's `.onion` address, where the box's iroh relay waits |
| 4 | iroh swaps addresses through that relay and both sides punch at the same moment |
| 5 | Until a direct path works, data goes through the relay over Tor |
| 6 | The app keeps punching. When a direct path works, multipath QUIC moves over without dropping any request |

| Layer | Technology | Job |
|---|---|---|
| Data | iroh (QUIC, multipath) over IPv6 or IPv4 | Full speed; survives network changes |
| Finding the box | BitTorrent DHT (Mainline), signed mutable items (BEP 44) | A signed record of the box's addresses, sealed for paired devices |
| Signalling | The box's iroh relay, as a Tor onion service | Reachable from anywhere, through any CGNAT, with no port opened |
| Fallback data | The same relay over Tor | Small traffic only; big transfers wait |
| Who gets in | The box's own device list, checked by connectd on every path and by the box's relay (iroh-relay's AccessControl) | Paired devices only; no passes, no clock, no fork of iroh |

## 4. Reach

Rough estimates until the beta boxes measure.

| Home internet | Device | Direct (no help) | Direct+ |
|---|---|:---:|:---:|
| Public IPv4, port opened | Any | ✅ | ✅ |
| Public IPv4, router refuses ports | Any | ❌ | ✅ punched, or Tor |
| CGNAT with IPv6, router lets the box in | Has IPv6 | ✅ | ✅ |
| CGNAT with IPv6, router blocks incoming IPv6 | Has IPv6 | ❌ | ✅ IPv6 punched |
| CGNAT with IPv6 | IPv4-only Wi-Fi | ❌ | ✅ punched, or Tor |
| CGNAT, no IPv6 | Another Wi-Fi | ❌ | ⚠️ 5–10% via Tor |
| CGNAT, no IPv6 | Mobile data | ❌ | ⚠️ 10–30% via Tor, less with [§8](#8-more-reach) |
| Strict router at both ends | Any | ❌ | ⚠️ via Tor |
| Public IPv4 | Network blocks UDP | ✅ TCP 443 forwarded | ✅ TCP 443, or Tor |
| CGNAT | Network blocks UDP | ❌ | ⚠️ via Tor |
| Any | Network blocks Tor | ❌ unless direct | ⚠️ direct, or Tor bridges |
| Any | Approved websites only | ❌ | ❌ |

⚠️ here means it works, with some connections through Tor. [§9](#9-hiding-tors-delay)
keeps families from noticing.

## 5. Work per component

| Where | Work |
|---|---|
| **connectd** | iroh relay on the box, checking the device list; Arti onion service in front of it (stable `.onion` address); DHT record (publish, refresh on address change, lookup keys changing daily); IPv6 first; PCP and UPnP pinholes; punching improvements ([§7](#7-punching)); dynamic DNS and own-domain updates ([§11](#11-names-and-certificates-for-browsers)) |
| **ZOOMRIYA OS** | Declared outbound connections for the DHT and Tor; firewall for iroh on UDP (IPv4 and IPv6) from paired devices |
| **Box Desktop** | Control Panel › Remote access: Direct+ on or off; how the box is reached now (direct or not); "Make remote access faster" ([§8](#8-more-reach)); domain or dynamic DNS setup; pairing phones and laptops |
| **Phone app** | Pairing at home; DHT lookup; Arti as iroh's proxy to the box's relay while open; race every path; adapt on Tor ([§9](#9-hiding-tors-delay)); "Check access from away" |
| **Laptop app** | The same as the phone app, plus the box served to the laptop's browser at `http://zoomriya.localhost:7480` ([§10](#10-laptops)) |

## 6. Pairing

Pairing needs no ZOOMRIYA server, no internet and no set clock (built in
phase 0).

| What | How |
|---|---|
| Phone | Box Desktop shows a QR code with the box key, a one-time secret and the home addresses. The phone connects over the home network and sends the secret, its device key and its link key, signed by the device key |
| Laptop | Box Desktop shows a short code (`4K7P-2QXM`, three tries, ten minutes). The laptop app finds the box over mDNS and proves it knows the code with SPAKE2, over iroh; the box proves it too before the laptop pins its key. The code itself is never sent |
| At home only | The box refuses pairing that does not come over the home network |
| Who gets in afterwards | The box's device list, on every path and every stream. Removing a device deletes it and cuts its connections at once |
| No passes | Nothing outside the box decides who gets in, so devices carry no pass and the box's clock does not matter |
| **Pair a phone** and **Add a laptop** | In Control Panel › Devices |

## 7. Punching

| # | Technique | Helps | Order |
|---|---|---|:---:|
| 1 | Same UDP socket for signalling and punching, kept alive every 10–15 s | Most CGNAT homes | 1st |
| 2 | Both sides told when to send; the box sends first | CGNAT at both ends | 1st |
| 3 | UDP 443 on the outside | Office and hotel networks | 1st |
| 4 | Detect double NAT; say "put your provider's modem in bridge mode" | UPnP that opens only the inner router | 1st |
| 5 | Remember per network what worked, try it first | Speed, fewer timeouts | 1st |
| 6 | **IPv6 first**: both send at once, opening a firewall that blocks incoming IPv6 | CGNAT homes with IPv6 | 2nd |
| 7 | Mobile data and Wi-Fi together | IPv4-only Wi-Fi, Wi-Fi that blocks UDP | 2nd |
| 8 | **Many ports at once**: about 256 sockets on the box, random ports from the phone, guided by CGNAT port blocks | Strict mobile CGNAT | 3rd |
| 9 | Keep the connection when the phone changes network (QUIC) | Drops on the move | Built in |
| 10 | Ask the CGNAT for a port (PCP) | A few providers | Any time |

## 8. More reach

| | Idea | Gain |
|---|---|---|
| A | **Router guidance and provider message.** Box Desktop › "Make remote access faster": turn on IPv6, allow the box in the IPv6 firewall, turn on UPnP, bridge mode for double NAT, with steps per router brand. CGNAT homes get a ready-to-send message: "Please give my home IPv6 or a public address" | Full speed for good, after one change |
| B | **Leave home already connected.** While still on home Wi-Fi with mobile data on, the phone and box signal over the home network and punch a path over mobile data; the connection stays up as the person leaves | Full speed from the door, until the connection drops (iOS in the background, a new mobile network) |
| C | **Tor bridges** (webtunnel, Snowflake), through Arti's pluggable transports | Works on networks and in countries that block Tor |
| D | **Single onion service.** The box does not need to hide: about 4 hops in total instead of 6 | Fallback about twice as fast |
| E | **Heavy work at home.** Backups and big transfers wait for home Wi-Fi or a direct path | Tor carries only small things |

## 9. Hiding Tor's delay

| Target | |
|---|---|
| Every tap | Answers at once (under 100 ms), from what is already on the device |
| New content from the box | Within about 1 s, no spinner first |
| Moving from Tor to direct | Unseen: no reload, no drop |

| How | |
|---|---|
| Act at once, sync after | Sent messages, ticked items and new events show immediately and go to the box in the background |
| Show what is already there | Recent chat, lists, the next two weeks of events, thumbnails, photos taken on the phone |
| Fetch ahead | On opening, the app asks the box what changed |
| Warm circuit | The Tor path is built when the app opens, not on the first tap |
| Myira streams | Word by word; Tor's extra second before the first word reads as her thinking |
| Send less on Tor | Thumbnails, then previews, originals when tapped; videos low quality or "plays when you are home"; backup waits; sync sends compressed changes only |
| Several circuits at once | Only if measurements show a real gain; capped, out of fairness to Tor volunteers |
| No wording about it | Never "slow" or "Tor"; big items show a thumbnail and fill in |

## 10. Laptops

| | |
|---|---|
| The app | **Only connects the laptop to the box.** It runs in the background (menu bar on a Mac, system tray on Windows and Linux), with a small menu: connected or not, pairing, Open ZOOMRIYA. Everything else is in the browser. Tauri with the phone's Rust core (`crates/link`, `crates/ffi`); macOS, Windows, Linux; signed; updates itself from zoomriya.com |
| Opening ZOOMRIYA | **Open ZOOMRIYA** opens the laptop's default browser at `http://zoomriya.localhost:7480` (the family apps) through a one-time link that sets the session cookie. Box Desktop is at `http://zoomriyabox.localhost:7480`, because it and the apps use the same paths; it gets its cookie through the apps' address. Both look and work as at home |
| Direct+ away | ✅ the same as the phone: Arti, DHT, iroh |
| Certificates | None needed: the app pins the box's key when it pairs |
| The laptop's own browser | The app serves the box at **`http://zoomriya.localhost:7480`**, at home and away, so bookmarks and sign-ins always work. Browsers keep `*.localhost` on the laptop and treat it as secure, so no certificate and no admin rights. Only those two names are answered (no DNS rebinding), only on loopback, and only with the session cookie (`HttpOnly`, `SameSite=Lax`, since the two names are separate sites), which keeps other programs and users on the laptop out |
| On the box | Each request goes on its own iroh stream to connectd, then as plain HTTP to the gateway's or Box Desktop's socket in `/run/zoomriya-link`, which only connectd and those servers can open. Box Desktop accepts the laptop's names only there |
| Pairing | The short code from Control Panel › Devices › **Add a laptop**. The app finds the box on the home network over mDNS (`_zoomriya-link._udp`, with the box key) and pairs with SPAKE2, so the box proves it knows the code before its key is pinned ([§6](#6-pairing)) |
| The device key | In a file only the user can read, until the macOS keychain, Windows' TPM and Linux's secret service hold it |
| At home with the app | The app connects to the box directly over the home network; the address stays the same |
| Laptop without the app | At home only: `zoomriya-xxxx.local`, after trusting the box once. Kept in the background |

## 11. Names and certificates for browsers

The apps are the main way in everywhere: the phone app, and
`zoomriya.localhost:7480` on laptops. **`zoomriya-xxxx.local` stays in the
background**, for devices that cannot install the app (a work laptop with
installs blocked, a Chromebook, a borrowed computer, a TV or tablet browser).

| `zoomriya-xxxx.local` | |
|---|---|
| Where it is mentioned | Only under "Without the app" in the user guide, and in Box Desktop's details |
| The apps | Still use it to find the box quickly at home |
| Work | None: already built |

A public authority never signs `.local` names, so browsers without the app need one of these:

| Option | Name | Padlock | Needs |
|---|---|---|---|
| **The box's own authority** (built) | `zoomriya-xxxx.local` | ✅ after trusting `ca.crt` once per browser | Nothing; the authority is name-constrained to `.local` and home addresses |
| **The family's own domain** | `home.smith.ca`, guests `share.home.smith.ca` (the family types only `smith.ca`) | ✅ Let's Encrypt, proved through DNS | A domain (about $10–20 a year) and their DNS provider's update key |
| **Dynamic DNS**, optional, off by default | `smith.<service>.org` | ✅ Let's Encrypt, where the service allows the DNS check | A free account at a dynamic DNS service; a token pasted in Box Desktop |

| What dynamic DNS and the own domain fix | |
|---|---|
| The home's public address changes | ✅ The box updates the records (A for IPv4, AAAA for IPv6) within minutes; short cache time, about 60 s |
| A home on CGNAT without IPv6 | ❌ The name points at a shared address that cannot reach the box |
| The apps | Already covered by the DHT; DNS is a second way to find the box |

| Concern | |
|---|---|
| Update key on the box | Readable by the certificate service only; Box Desktop recommends a key limited to these records |
| Routers that refuse private DNS answers | `home.smith.ca` fails at home; the box falls back to `zoomriya-xxxx.local` |
| A dynamic DNS service closing | The name is gone; the apps keep working |

## 12. Guests

Guests have only a browser: no app, no Tor, no DHT.

| Home | Guest link |
|---|---|
| Public IPv4, door opened | ✅ through the door, to the album viewer only |
| CGNAT with IPv6, door opened | ✅ for guests with IPv6 |
| CGNAT without IPv6 | ❌ send the photos, or an onion link |

| | |
|---|---|
| Name | The family's own domain or dynamic DNS. Without either: a bare address, with a Let's Encrypt certificate for the home's public address if available (to check) |
| Outside check | The box checks its own door **through Tor**, as a stranger would, so only the album viewer can be reached |
| Onion link | The album viewer as its own onion service, one per share link. Works from any home, but only in Tor Browser or Brave's Tor window. Offered only when the door cannot work, as "Link for Tor Browser" |
| Send the photos | The app exports the album as files, for any messenger. Always works |

## 13. Encryption and security

| Path | Encryption | Who can read it |
|---|---|---|
| App ↔ box, direct | QUIC with TLS 1.3, keyed to the box's and device's keys, pinned at pairing | Device and box only |
| App ↔ box, through Tor | Tor's end-to-end encryption, and inside it the same iroh encryption | Device and box only; Tor relays see nothing readable |
| DHT record | Sealed with ChaCha20-Poly1305 and signed with a key that changes daily, both made from a secret the box gives each device at pairing. The box's key is never in the DHT, so nobody can tell which records belong to the same box | Paired devices only |
| Laptop app ↔ laptop's browser | Plain HTTP on `zoomriya.localhost:7480`, never leaves the laptop, session token | That laptop only |
| Browser ↔ box | HTTPS; certificate keys are made on the box and never leave it | Browser and box only |
| Guest ↔ box | HTTPS through the door, or Tor through an onion link | Guest and box only |

| Weak point | |
|---|---|
| **Whoever controls DNS** (the family's DNS provider, a dynamic DNS service, or someone who breaks in) | Can point the name elsewhere and get a valid certificate for it, so a browser shows a padlock on a fake page |
| Not affected | The apps (they pin the box's key), `.local` and onion links |
| What helps | A CAA record allowing only the box's Let's Encrypt account; the box watches the public certificate logs for certificates it did not request and warns in Box Desktop |

Data stored on the box is unchanged by any of this.

## 14. What Direct+ cannot do

| | |
|---|---|
| Networks that allow only approved websites | Nothing gets through; mobile data is the answer |
| Guests reaching a CGNAT home without IPv6 at full speed in any browser | Needs a server; Direct+ offers "send the photos" or an onion link |
| A promise of full speed everywhere | Some connections run through Tor; [§9](#9-hiding-tors-delay) hides it |
| Reaching the box when the home internet is down | Nothing does, without a SIM; the apps show their copies |

## 15. Phases

**Laptop first, then phone.** Arti, iroh and Tauri are easiest to build and
debug on a desktop, so the laptop proves the whole stack before the phone.
All work happens on the `direct+` branch in zoomriya-connect, zoomriya-os,
zoomriya-apps, zoomriya-docs and zoomriya-id; `main` stays as it is.

| Phase | Goal | Work | Where | Done when |
|---|---|---|---|---|
| **0. Foundations** ✓ | The box with no ZOOMRIYA server | ZOOMRIYA Connect's servers, their GUI, the extension and the box's code for them removed; passes and the clock gone; pairing with no name, no internet and no clock ([§6](#6-pairing)); the link always on; laptops pair with the short code over iroh | `connectd/src/api.rs`, `daemon.rs`, `devices.rs`, `link.rs`; `crates/proto`, `crates/keys` | A phone and a laptop pair with a box that has no ZOOMRIYA server and no internet (`connectd/src/e2e.rs`) |
| **1. Laptop at home** ◐ | The laptop app works on the home network | Tauri tray app: pairing with the short code, iroh to the box at home, `zoomriya.localhost:7480` in the browser; **Add a laptop** in Control Panel › Devices | New `laptop/`; `crates/link`; `connectd/src/link.rs`; Box Desktop `connect.go` and `web/` | A paired laptop opens Box Desktop and the family apps at `zoomriya.localhost:7480`, no trust step, no ZOOMRIYA Connect. Built and tested on Linux: the tray app (`laptop/app`, Tauri), pairing over mDNS and SPAKE2, both addresses in the browser, one copy at a time. Left: a run on a real box with real browsers (checks 10–12), macOS and Windows builds, signing and updates |
| **2. Laptop away, direct** ◐ | Reach homes that can be reached | Sealed DHT record and lookup; remembered addresses; stable IPv6, PCP, NAT-PMP and UPnP port mapping, the outside address, double NAT detection | `connectd/src/reach.rs`, `net.rs`; `crates/keys/src/record.rs`; `crates/link/src/dht.rs`, `client.rs`; `laptop/` | Away, the laptop reaches a home with public IPv4 or open IPv6, after the home's address changed. Built and tested on Linux: the box publishes its record for today and tomorrow, every 30 minutes and when its addresses change; the laptop gets the secret at pairing, finds the record and connects through it (`zoomriya-laptop check-away`). connectd's status reports the reach for Box Desktop ([phase 5](#15-phases) shows it). Left: the same from another network ([check 13](#16-prototype-checks)) |
| **3. Laptop away, Tor** ◐ | Reach every home | `crates/tor` (Arti); the box's iroh relay behind an onion service; Tor fallback in the link; punching through the box's relay (items 1–5); keep punching and move over (multipath); declared connections and firewall in the OS | `crates/tor`; `connectd/src/relay.rs`, `daemon.rs`; `crates/link`; `laptop/`; `packages/zoomriya-connect` | From a CGNAT home without IPv6, the laptop connects through Tor, then moves to direct without a reload. [Prototype checks](#16-prototype-checks) 1–4 pass on a laptop. Built and tested on Linux: connectd runs iroh's relay on 127.0.0.1, letting in only the box and paired devices, behind an Arti onion service whose key stays in the box's data folder. The box gives its onion address at pairing. The laptop app reaches the relay through a local proxy that goes only to that address, and races it against the direct tries. `zoomriya-laptop check-tor` reached the box through Tor in 10–14 s and moved to a direct path 0.5–1.5 s later; a laptop that was not paired was turned away by the relay. Left: a run from a home with no direct path, check 4, and punching beyond what iroh does itself (items 1–5) |
| **4. Laptop, unnoticed Tor** ◐ | Families never notice Tor | Warm circuit; "on Tor" signal to the apps; apps adapt (thumbnails, backup waits, act at once and sync after); heavy work at home; single onion service; Tor bridges; IPv6 first; many ports at once | `crates/link`, `crates/tor`; `laptop/`; zoomriya-apps `packages/web`, `myapps/photos`, `chat`, `planner` | The [§9](#9-hiding-tors-delay) targets met on a laptop through Tor. Built: the laptop app connects when it starts and reconnects when the connection drops; every answer carries `Zoomriya-Path: direct` or `relay` (and `/.zoomriya/path`); the apps' shared `connection.svelte.ts` follows it. On the relay, Photos shows the thumbnail before the larger picture, a video waits for a direct path, and big uploads wait (`whenFast`); Chat shows plain messages at once and sends them in the background; Planner already ticked at once. Bridges: the family's own lines in the laptop app's `bridges.txt`, or the public Snowflake bridges when Tor has not joined within 30 s and `snowflake-client` is installed (Arti's transports, switched while running). IPv6 first: iroh already sends from both sides at once, through the relay. Left: the targets measured from a network where only Tor gets through; bridges on a network that blocks Tor; many ports at once (§7 item 8), which needs its own punching beside iroh's. A single onion service waits for Arti ([check 6](#16-prototype-checks)) |
| **5. Box Desktop** ✓ | Families can see and improve their access | Control Panel › Remote access: on or off, how the box is reached, "Make remote access faster" with router steps and the provider message | `packages/box-desktop`; `docs/box-desktop.md`; `connectd/src/away.rs`, `api.rs` | An admin sees the home's case and the fix for it. Built: connectd keeps the switch (`away.json`; off stops the record, closes connections from away at once, and the relay lets in the box only; tested for real through Tor); its status gives the reach, the port and the switch. Box Desktop shows the case (a port the router opened, IPv6, two routers, a shared address), the paths row by row, and only the steps that fit, with the provider message to copy; French drafts. Router steps are general, not per brand |
| **6. Phone** | Everything above on phones | The phone app (Swift, Kotlin) on `crates/ffi`; QR pairing; Arti on iOS and Android; mobile data and Wi-Fi together; **leave home already connected**; "Check access from away"; **Pair a phone** in Control Panel › Devices | `crates/ffi`, `crates/link`; the phone app (new) | A phone does phases 1–4 at home and away; iOS check 5 passes |
| **7. Names and guests** | Real padlocks and guest links | Family's own domain and dynamic DNS (records and certificates); guest links through the door; onion links; outside check through Tor; CAA record and certificate log watch | `connectd/src/dns.rs`, `acme.rs`; `zoomriya-agent`; Box Desktop | A guest opens `share.home.smith.ca`; `home.smith.ca` has a real padlock at home |
| **8. Finish** | Clean up and measure | OS and user guide docs (`.local` under "Without the app"); bring `compare.md` in line; anonymous opt-in measurements | All repos | Docs match the code; the beta reports how often each path is used |

| Phase | Depends on |
|---|---|
| 1 | 0 |
| 2 | 1 |
| 3 | 2 |
| 4 | 3 |
| 5 | 2 (grows with 3 and 4) |
| 6 | 4 |
| 7 | 2 |
| 8 | 6 and 7 |

## 16. Prototype checks

| # | Check | Why |
|---|---|---|
| 1 | iroh reaches its relay through Arti's proxy on the phone | ✓ on a laptop (iroh's HTTP CONNECT proxy, to a proxy in front of Arti); the phone is still to check |
| 2 | The iroh relay runs well behind an Arti onion service on the box | ✓ on a laptop standing in for the box: the descriptor is published 10 s after start |
| 3 | Time from opening the app to a direct path, through Tor | Target under 5 s. Measured 11–15 s from a cold start, almost all of it reaching the onion service: phase 4's warm circuit and racing the direct tries hide it |
| 4 | Speed and delay of the fallback through a warm circuit | Sets what the app must adapt |
| 5 | Arti in the iOS app, while open | Background limits |
| 6 | Single onion services and path weighting in Arti | §8 D. Not in Arti 0.48 (still a TODO in its onion service settings); look again at each Arti update |
| 7 | Let's Encrypt certificates for a home's public IP address | Guest links without a domain |
| 8 | Safari treats `zoomriya.localhost` as secure | Chrome and Firefox do |
| 9 | How often each path is used, from the beta boxes (anonymous, opt-in) | Real numbers replace the estimates |
| 10 | connectd's mDNS announcement beside avahi on the box | Both answer on UDP 5353; connectd uses its own host name |
| 11 | Apps in Box Desktop's windows on a laptop | `zoomriyabox.localhost` and `zoomriya.localhost` are separate sites, so an app framed in Box Desktop is a third-party frame: it may ask to sign in, or need to open in its own tab |
| 12 | WebSockets and live updates through the laptop app, on a real box | The apps' chat and Box Desktop's events |
| 13 | The laptop on another network (a phone's hotspot) reaches the box through its DHT record | Phase 2 was tested from the same home: the record was found in the DHT and the connection worked, but over the home network |

## 17. Open questions

| Question | Options |
|---|---|
| Wording | What Box Desktop and the apps say for each state, with Tor never named |
| Which dynamic DNS services and DNS providers to support first | — |
