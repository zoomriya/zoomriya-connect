# Starting without a relay

Part of [ZOOMRIYA Connect](../README.md). Notes from the discussion of
22 November 2026. **Not decided in full yet.** The other pages still
describe the design with a relay until this one is agreed.

## The decision so far

| | |
|---|---|
| At launch | No relay and no TURN. Our servers never carry a family's data |
| Later | A relay may come back if ZOOMRIYA succeeds |
| What stays | A small **meeting point**: it passes sealed connection offers so both sides can send at the same moment. It never carries data, and it can't read the offers |
| Hole punching | WebRTC or iroh, whichever works best. Either is fine |
| Why | Families don't want to depend on our relay, and the cost stays simple |

## What works without a relay

Rough estimates from published figures, before our own tests. The beta boxes'
measurements replace them.

| Situation | Works | Fails |
|---|:---:|---|
| At home, internet up or down | ✅ | 0% |
| Away, home has its own public address and the box opened its port | ✅ | ~0–2% |
| Away, home has its own public address, router refuses to open ports | ✅ | ~5–10% |
| Away, network blocks UDP, home has its own public address | ✅ | Low (TCP 443 on the box) |
| Away, network blocks UDP, home on CGNAT | ❌ | 100% |
| Away on Wi-Fi elsewhere, home on CGNAT | ✅ | ~5–10% |
| Away on mobile data, home on CGNAT | ⚠️ | ~10–30% (strict mobile CGNAT) |
| Both sides strict (symmetric NAT), no IPv6 | ❌ | 100% |
| Home and phone both on IPv6 | ✅ | ~0–2% |
| Guests, home has its own public address | ✅ | ~0–2% |
| Guests, home on CGNAT | ⚠️ | ~10–30% |
| Meeting point down: new connections from away | ❌ | Until it is back. Home use and open connections keep working |
| Networks that allow only approved websites, or inspect encrypted traffic | ❌ | 100% (a relay wouldn't help either) |
| Your own server or Cloudflare Tunnel | ✅ | ~0% |

Overall: about 5–15% of connections from away fail, nearly all from homes on
CGNAT. The improvements below should bring that down to about 2–6%.

## Improving direct connections

| # | Technique | Helps | Effort |
|---|---|---|---|
| 1 | **IPv6 first** (below) | CGNAT homes, strict mobile CGNAT | Medium |
| 2 | The box talks to the meeting point from the same UDP socket it punches with, and keeps the mapping alive | Most CGNAT homes; no separate STUN server needed | Small |
| 3 | The meeting point tells both sides to send at the same moment; the box sends first | CGNAT on both sides | Small |
| 4 | Port guessing: one side opens many ports, the other sends to many guessed ports. CGNATs give each home a block of ports, which makes guessing easier | Strict mobile CGNAT with an easy home | Medium |
| 5 | Phones try mobile data and Wi-Fi at the same time | Wi-Fi that blocks UDP | Small |
| 6 | UDP 443 outside, the port HTTP/3 uses | Office and hotel networks | Small |
| 7 | Detect double NAT; setup says "put your provider's modem in bridge mode" | UPnP that opens only the inner router | Small |
| 8 | Ask the CGNAT for a port with PCP | The few providers that allow it | Small |
| 9 | Remember per network what worked, and try it first | Speed, fewer timeouts | Small |
| 10 | Keep the connection when the phone moves between networks | Drops on the move | Built into QUIC |

Order: 2, 3, 6, 7, 9 first (small, most homes), then 1 and 5, then 4.

## IPv6

IPv6 has no address translation, so each side knows the other's real address
and port. When both send at the same moment, each firewall sees its own
outgoing traffic and lets the reply in. That works even when the router blocks
incoming IPv6 and offers no way to open it.

| Step | What happens |
|---|---|
| 1 | The box uses a **stable** global IPv6 address, not a temporary one |
| 2 | It tries to open the router's firewall with PCP, then UPnP's IPv6 firewall control. A bonus when it works, not needed |
| 3 | It gives the meeting point its IPv6 address and port, sealed for the family's devices |
| 4 | The phone sends its own through the meeting point; both send at once |
| 5 | Phones race IPv6 and IPv4, and keep the first that answers |
| 6 | Keep-alives every 10–15 s hold the firewall opening |
| 7 | If the provider changes the home's prefix, the box updates the meeting point at once |

| Where | Work |
|---|---|
| connectd | Stable address, pinhole through PCP or UPnP, prefix watch, outside check through the meeting point |
| ZOOMRIYA OS firewall | UDP 443 on IPv6, from paired devices only |
| Meeting point | Listens on IPv6 and IPv4, passes both sides' addresses |
| Phone and laptop | Race IPv6 and IPv4 (iroh does already; WebRTC too) |

The home's IPv6 address only ever goes, sealed, to the family's own paired
devices. It is never published.

IPv6 in Canada: about 30–43% of users, depending on who measures; about 50%
worldwide (March 2026). Providers that use CGNAT usually also have IPv6.

## Test, 22 November 2026

One home and one mobile carrier, from one computer with two connections: the
home's Ethernet, and an iPhone's Personal Hotspot over USB.

**The home:**

| Check | Result |
|---|---|
| Router | The provider's (Sagemcom Fast 5689), IPv6 on, UPnP on |
| IPv4 | CGNAT: the router's internet-side address is 10.130.x.x |
| Incoming IPv6 | Blocked, with no setting to allow it |
| UPnP for IPv6 | Not offered (only `WANPPPConnection`, for IPv4) |
| PCP, NAT-PMP | No answer |
| Mobile data | IPv6 (test-ipv6.com 9/10) |

**The test**, UDP between the home address and the mobile address:

| Phase | Received at home | Received on mobile |
|---|---|---|
| 1. Mobile sends, home quiet (20 s) | 0 of 40 | — |
| 2. Both send at once (20 s) | 40 | 40 |
| 3. Home stops, mobile keeps sending (15 s) | 30 | — |

Hop limits (241 at home, 50–51 on mobile) show the packets crossed the
internet.

**Result:** in a home with CGNAT, a router that blocks incoming IPv6 and no way
to open it, IPv6 hole punching works with no relay. The opening stayed open for
at least 15 s with no traffic from home.

Still to test: other providers' routers, other carriers, the real phone app
(not a hotspot), and how long openings last on each.

## The meeting point

**Decided (22 November 2026): our relay server with a byte limit per device**
(option A), not a new signalling server. It is called the **meeting point**
everywhere people read about it (website, Box Desktop, docs), never "relay".
Code keeps iroh's names (`iroh-relay`, the `relay/` crate).

It introduces the phone and the box so they can connect directly, then steps
out:

| Step | What happens |
|---|---|
| 1 | The box stays connected to it, with a keep-alive every 15–25 s |
| 2 | The phone connects to it when the app opens away from home |
| 3 | It tells each side its own public address (iroh's QAD, as STUN does) |
| 4 | The phone's offer goes through it to the box, sealed |
| 5 | The box answers; both sides are told to send at once |
| 6 | Both send together; each router lets the reply in |
| 7 | Connected directly. Nothing more goes through the meeting point |

| How it's built | |
|---|---|
| Software | iroh-relay as today, no fork. `AccessControl` still checks passes |
| Byte limit | iroh-relay's per-device rate limit, set very low, enforced in the front door that already meters bytes |
| No direct path | connectd refuses app data until the path is direct. The app says "can't reach home from this network" instead of crawling |
| Laptops and guests | Their WebRTC offers already go through this server |
| Families' own | "Just a relay" becomes "your own meeting point": same software, same one-time code in Box Desktop |

| Sees | Never sees |
|---|---|
| Which device and which box, when, and both internet addresses | Content, the sealed offers, or how much the family uses |

| If it is down | Effect |
|---|---|
| At home, open connections | Nothing changes |
| New connection to a home reachable directly | Works: the phone dials the addresses it remembers |
| New connection to a CGNAT home | Fails until it is back |

Website wording: "It passes only the first few kilobytes that set up the
connection, encrypted. Your photos, messages and files never go through it."

| Also decided | |
|---|---|
| Byte limit | About 2 KB/s per device, with a 64 KB burst: enough for setup, useless for data |
| Servers | One at launch, on the control plane's machine. A second, with another hosting company, before the beta grows; phones and boxes try it if the first is down |

## Port guessing

For IPv4 when one side is behind a **strict** router (a new public port for
each destination) and the other is **easy**. Usually a phone on a mobile
carrier's CGNAT. IPv6 doesn't need it.

**Decided (22 November 2026): methods 1 and 2 at launch.** Methods 3 and 4
only if the beta's numbers show strict carriers are a real problem.

| # | Method | When it works | Cost | When |
|---|---|---|---|---|
| 1 | **Same port:** try the device's own port number | Strict routers that keep it | 1 packet | Launch |
| 2 | **Next port:** ask the meeting point twice, see the step (+1, +2), predict the next | Routers that hand out ports in order | A few packets | Launch |
| 3 | **Port block:** guess inside the customer's CGNAT block (512–4,096 ports) | Many carrier CGNATs | ~20–100 packets | Later, if needed |
| 4 | **Many at once:** the strict side opens 256 ports, the easy side sends to random ports (~98% within 1,024 packets) | Any strict + easy pair | ~1,000 packets, 5–10 s | Later, if needed |

| At launch | Work |
|---|---|
| Router type | Detected through the meeting point: two server addresses (a second IP on the meeting point's server, or the control plane's address) show whether the public port changes per destination |
| connectd and the app | Methods 1 and 2, tried alongside the normal punching |
| Measurements | Each device's router type and whether the connection worked, to decide on 3 and 4 |
| Limits | Only to the address the meeting point gave, only after the pass check, a handful of packets |

iroh and WebRTC do neither 3 nor 4: they would need our own code to find the
port, then QUIC on that socket.

## Guests of CGNAT homes

A guest has only a browser, installs nothing and has no pass.

**Decided (22 November 2026):** for homes the guest's browser can't reach,
**our server serves the empty album viewer page** (layout and code, never a
photo). The page reaches the box through the meeting point and hole punching
(WebRTC, IPv6 or IPv4, with the box's predicted ports), and the photos go
straight from the box to the browser. The link's secret part (after `#`) holds
the share key and the box's key fingerprint and never reaches our server. Homes
with their own public address keep the guest door on the box, as today.

| Our server sees | Never sees |
|---|---|
| That a viewer page was opened, and the guest's internet address | Photos, the share key, or which album |

Trust point: the viewer page's code comes from our server (open source).
Families who don't want that can use their Cloudflare Tunnel or their own
server.

Rough estimate: about 85–95% of guests of CGNAT homes can open an album. The
rest are on networks that block UDP, or strict on both sides with no IPv6.
They see: "This album can't be opened from your network right now. Try mobile
data or another Wi-Fi."

Dropped (22 November 2026): a "Send copies instead" fallback in the phone app.
Copies would leave ZOOMRIYA for third-party services and couldn't be taken
back, unlike a share link. Reconsider only if beta families ask for it.

## Setup check

Runs by itself in first setup (about 10 s, alongside other steps) and **never
blocks setup**.

| # | Check | How |
|---|---|---|
| 1 | Cable, same network as the Wi-Fi | As today |
| 2 | Router's internet-side address | UPnP, NAT-PMP or PCP |
| 3 | Own public address or CGNAT | Router's address against what the meeting point sees; private, 100.64.x.x or a mismatch means CGNAT |
| 4 | Double NAT | Two routers in a row |
| 5 | Port opened | UPnP or PCP, then the meeting point tries it from outside |
| 6 | Easy or strict router | The meeting point's two addresses |
| 7 | IPv6 | Global address; reachable uninvited; or working when the box sends first |

Results (decided 22 November 2026):

| Result | When | Wording |
|---|---|---|
| ✅ From anywhere | Own public address with the port open, or IPv6 open | "You can reach your box from anywhere." |
| ✅ From most places | CGNAT or closed router, IPv6 punching works | "You can reach your box from most places." |
| ⚠️ From some places | CGNAT, no IPv6, easy router | "You can reach your box from many places, but not from some mobile and work networks." |
| ⚠️ Hard from away | Strict router, no IPv6 | "Your box may be hard to reach from away." |
| ❌ Home only | Remote access off, or no internet | "Your box works at home only." |

| Cause | Fix shown |
|---|---|
| Double NAT | "Your home has two routers. Put your provider's modem in bridge mode." |
| IPv6 off on the router | "Turn on IPv6 in your router's settings." |
| UPnP off, own public address | "Turn on UPnP in your router's settings." |
| CGNAT, no IPv6 | "Ask your internet provider for IPv6 or a public address." |

Technical words (CGNAT, addresses, router type) only in a Details line. Shown
in first setup and Box Desktop › ZOOMRIYA Connect (with Check again).
Decided: re-check daily and when the home's address changes; the bell tells
admins only when the result gets worse.

**Decided (22 November 2026): "Check access from away" in the phone app, for
admins only.** At home: turn off Wi-Fi, tap Check. The phone connects over
mobile data the way it would from anywhere, and shows whether it reached the
box (and over IPv6 or IPv4, in Details). The result also shows in Box Desktop.

## When a phone can't reach home

**Decided (22 November 2026).** Nothing is lost and nothing spins.

| Across the app | Behaviour |
|---|---|
| Status | A small line at the top: "Not connected to home · Trying again", gone once connected |
| Something that needs the box | At once: "Can't reach your box from this network. It will work on another Wi-Fi or at home." |
| Retrying | On every network change and about every minute, by itself |
| Notifications | Still arrive (the box sends them out through Apple or Google); tapping one shows the message above |

| App | Waits on the phone, sent later | Shown from the phone | Needs the box |
|---|---|---|---|
| Family Photos | Backup of new photos, resumed | Photos taken on this phone | The family library |
| Family Chat | New messages, in order | Private chats; the last 100 messages of each family chat | Older family messages |
| Family Planner | Ticks and additions to lists | Shopping and to-do lists; the next 2 weeks of events | Other new events |
| Family Vault | — | **Never kept on the phone** | Everything |
| Family Health | "Mark as taken", with the real time | Reminders already scheduled | Records (never kept on the phone) |
| Family Tracker (dropped for now, 22 November 2026; for when it returns) | Own locations, buffered with real times | Last places, with "seen 10:42" | Live updates |
| Myira | — | — (conversations never kept on the phone) | Everything |
| Family Inbox | — | — | Everything |

Copies kept on the phone stay in the ZOOMRIYA app's encrypted storage on the
family's own device, and are deleted when the phone is removed from the box.

When Family Tracker returns, its silence alert says "Not heard from since
3:40 pm", not "No signal" (changed in the Tracker docs the same day).

## Open questions

| Question | Notes |
|---|---|
| Guests | Without TURN, guests of CGNAT homes need IPv6 or WebRTC hole punching |
| Website and pricing | The site still describes the relay, and the yearly plan included it |
| Measurements | Per-device results on the box, anonymous opt-in to us, to replace the estimates |
