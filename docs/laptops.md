# Laptops: the browser extension

Part of [ZOOMRIYA Connect](../README.md).

On a laptop, the ZOOMRIYA browser extension lets you use the box from
anywhere, in every option: with ZOOMRIYA Connect, and with
[your Cloudflare Tunnel](sharing-and-names.md#your-cloudflare-tunnel).
There's nothing else to install, and no admin rights are needed.

Phones use the ZOOMRIYA app, as before.

## Using it

The same three steps in every browser:

| Step | What you do |
|---|---|
| 1 | Install the ZOOMRIYA extension from your browser's add-on store |
| 2 | At home, click the ZOOMRIYA button, type your box's name and the code from Box Desktop (**Add a laptop**), and pick **Pair** |
| 3 | Anywhere, click the ZOOMRIYA button and pick **Open ZOOMRIYA** |

What happens behind the scenes differs between browsers (below), never what
you do.

## Browsers

| Browser | Package | Store | State |
|---|---|---|---|
| Chrome, Brave, Vivaldi | `chromium` | Chrome Web Store | Works, at home and away |
| Microsoft Edge | `edge` | Edge Add-ons | Same build as Chrome |
| Opera | `opera` | Opera add-ons | Same build as Chrome |
| Firefox (140 and newer) | `firefox` | Firefox Add-ons | Works, at home and away: see [Firefox](#firefox) |
| Safari | None yet | App Store, inside a small Mac app | Not tested: needs a Mac |

`npm run build` in `extension/` makes every package, as a folder in
`extension/dist/<package>/` and a zip in `extension/dist/packages/`. One build
serves them all; each package leaves out the other browser's files.

| | Chrome family | Firefox |
|---|---|---|
| Where the box's pages open | The extension's own address (`chrome-extension://…/`) | Box Desktop's own address (`https://<box name>:7443/`) |
| What answers their requests | The extension's service worker (`src/sw.ts`) | The box's service worker (`zr-sw.js`, served by Box Desktop), which hands them to the extension |
| Where WebRTC runs | A hidden offscreen page (`src/offscreen.ts`) | The extension's background page (`src/firefox/background.ts`) |
| The WebRTC connection, keys, cookies, headers | `src/lib/conn.ts`, `keys.ts`, `cookies.ts`, `headers.ts`: the same code | The same code |
| Manifest | V3 | V2 (see [Firefox](#firefox)) |

## The checks

| Check | What we need to see | Result |
|---|---|---|
| 1. The box's pages in the extension | Box Desktop and the box's apps run through the extension: sign-in, cookies, the box's `Origin` checks, live connections | **Passed** in Chromium (from the extension's address) and Firefox 153 (from Box Desktop's address, through the box's service worker): sign-in, cookies, `Origin` checks and Box Desktop's live event stream. WebSockets: not needed by Box Desktop, not yet built |
| 2. Connections from the extension | The extension reaches the relay with its pass and sets up WebRTC through it. In Chrome, WebRTC needs a hidden extension page; in Firefox it runs in the background page | **Passed** on one computer, in Chromium and Firefox: at home, WebRTC connects in about 10 ms; away, the offer goes through the relay and the answer comes back sealed. Real networks: check 3 |
| 3. Direct connections | In the [network lab](testing.md#stage-2-the-network-lab), WebRTC reaches the box directly about as often as iroh does | Not run yet |

## How it's built

| Part | Where | What |
|---|---|---|
| Popup | `extension/src/popup/` | Pairing with the code, opening the box, forgetting it |
| The connection | `extension/src/lib/conn.ts` | WebRTC: signs the offer, sends it home or through a relay, checks the box's answer against the pinned box key, sends the box's pages' requests on the data channel. Both browsers |
| Cookies and headers | `extension/src/lib/cookies.ts`, `headers.ts` | The box's cookies, kept by the extension. What goes to the box with a request and what of its answer reaches the page. Both browsers |
| Service worker (Chrome family) | `extension/src/sw.ts` | Answers every request of the box's pages, under the extension's address, by sending it to the offscreen page |
| Offscreen page (Chrome family) | `extension/src/offscreen.ts` | Runs `conn.ts`: Chrome allows WebRTC only in a page |
| Background page (Firefox) | `extension/src/firefox/background.ts` | Runs `conn.ts`, keeps the hidden bridge frame, answers the box's service worker, checks the box's certificate |
| The box's service worker and bridge (Firefox) | zoomriya-os: `packages/box-desktop/web/public/zr-sw.js`, `zr-bridge.html`, `zr-bridge.js` | Served by Box Desktop. See [Firefox](#firefox) |
| Keys | `extension/src/lib/keys.ts` | The device key (P-256) and link key (Ed25519), made by the browser, never exportable, kept in its own database |
| Packages | `extension/scripts/package.mjs` | Each browser's manifest and zip |
| Messages | `crates/proto/src/laptop.rs` | Pairing, the offer and answer, and the frames on the data channel |
| Box side | `connectd/src/laptop.rs` | Pairing and connecting at home, WebRTC for paired laptops, requests passed to Box Desktop with the laptop's label (PROXY protocol v2). Waits on each relay for offers from away, while any laptop is paired |
| Relay side | `relay/src/laptops.rs` | Passes offers from away to their box and the box's answers back. Checks the laptop's pass and link key, never reads the offer |

### Connecting

| Step | At home | Away |
|---|---|---|
| 1 | The extension makes a WebRTC offer, signed with its device key | The same. STUN tells WebRTC the laptop's public address, and TURN credentials are ready in case nothing direct works |
| 2 | It sends the offer to the box's home address (TCP 7445). It waits 3 seconds at most | Nothing answers at home, so it sends the offer to the first relay, with its pass and a signature by its link key |
| 3 | | The relay checks the pass and passes the offer to the box, which is waiting on the relay for offers |
| 4 | The box checks the device key's signature and answers, sealed by the box key, with a fresh pass | The same, back through the relay. The box offers its public address too, which STUN on its WebRTC port told it |
| 5 | WebRTC connects on the home network | WebRTC connects straight to the box through both routers, or through TURN if it can't ([below](#when-the-direct-path-fails)). The relay never carries the box's pages |

The pass comes fresh with every answer, so a laptop used at least once a
month never needs pairing again.

### When the direct path fails

Some networks block WebRTC's direct path: hotel, airport, office and school
Wi-Fi that blocks UDP, or routers that change addresses in ways hole punching
can't follow. Then WebRTC falls back to **TURN**, a server that passes the
encrypted traffic between the laptop and the box. It runs beside each relay
([TURN beside a relay](operations.md#turn-beside-a-relay)).

| What | How |
|---|---|
| Credentials | The box asks its relay for them (signed with the box key), keeps them while more than half their day is left, and sends them in every answer, sealed with the rest. The laptop keeps them for its next connection |
| Order | WebRTC tries direct first. TURN only when nothing direct works: UDP 3478, then TCP 3478, then TLS on port 443 for networks that allow nothing but the web |
| Out-of-date credentials | If the connection fails with the credentials kept from last time and the answer brought new ones, the extension tries once more with them |
| Privacy | TURN passes WebRTC's encrypted traffic. It can't read or change the box's pages, as with the relay |
| Bandwidth | Counted per box from the TURN server's own records, in the box's monthly allowance, and on the GUI's Servers page |

| Port on the box | From | What |
|---|---|---|
| TCP 7445 | The home network only | Pairing and setting up a connection at home, over HTTPS with the box's certificate |
| UDP 7446 | Anywhere | WebRTC. connectd answers only offers signed by a paired laptop |

Every answer from the box is sealed by the box key, and the extension checks
it against the key it pinned at pairing. The answer names the offer it
answers, so WebRTC's encryption is tied to the real box: nobody on the
network can step in, even with a certificate for the box's name.

## Firefox

Firefox lets extensions run no service worker of their own (Mozilla's bug
1573659), and nothing else in an extension can answer the requests a page
makes to the extension's own address. So in Firefox the box's pages open at
Box Desktop's real address, and the **box's own service worker** answers
them, handing each request to the extension. For the person using it, the
steps are the same as in Chrome.

### How it works

| When | What happens |
|---|---|
| Pairing, at home | After the usual pairing, the background page opens a hidden frame of `https://<box name>:7443/zr-bridge.html`. The box serves that page; it installs the box's service worker, `zr-sw.js`, for Box Desktop's address. The popup waits for it (a second or less) before saying **Paired** |
| Every start of Firefox | The background page opens the hidden frame again. At home it updates the service worker; away, the worker installed earlier serves the frame from its own copy |
| Open ZOOMRIYA | The popup checks the frame is ready, then opens `https://<box name>:7443/` in a tab |
| Each request of the page | The box's service worker sends it to the hidden frame, the frame to the background page, the background page to the box over WebRTC (`conn.ts`, as in Chrome). The answer streams back the same way |
| Away from home | The box's address doesn't answer, but the service worker answers before anything goes to the network. Nothing goes through the relay except the WebRTC offer and answer |
| No extension, or it's off | The service worker sends requests to the network as usual: at home, straight to the box. Away, a page says to turn the extension on |

### The parts

| Part | Where | What |
|---|---|---|
| `zr-sw.js` | Box Desktop (`web/public/`) | The box's service worker for its own address. Keeps copies of the bridge files. Sends requests to the bridge, or to the network without one. Refuses forms posted as page loads |
| `zr-bridge.html`, `zr-bridge.js` | Box Desktop (`web/public/`) | The bridge: registers the worker, passes its requests to the frame's parent, the extension. Served with `frame-ancestors moz-extension:`, so only an extension can frame it, never a website |
| Background page | `extension/src/firefox/background.ts`, `zr/background.html` | Keeps the bridge frame (again every minute until it answers), checks every message comes from it, runs the request through `headers.ts`, `cookies.ts` and `conn.ts` |
| Popup | `extension/src/popup/` | After pairing and before opening, asks the background page `setup` and waits for the bridge |
| Box Desktop's address for laptops | `LaptopPaired.desktop`, from connectd's `[laptops] desktop_url` (default `https://<box name>:7443`) | Where Firefox opens the box |
| The box's certificate key | `LaptopPaired.tls_keys` and `LaptopAnswer.tls_keys`, sealed by the box key | What Firefox checks every page against |

### The messages

| From → to | Message |
|---|---|
| Service worker → bridge | `{t: "request", v: 1, method, path, headers, body, navigate, sameOrigin}` with a `MessagePort` for the answer |
| Bridge → extension | The same message and port, unchanged (`postMessage` to the parent) |
| Bridge → extension | `{t: "bridge-ready", v: 1}` once the worker is active |
| Extension → service worker (on the port) | `{t: "response", status, headers}`, then `{t: "chunk", bytes}`s, then `{t: "end"}`; or `{t: "error", message}` |
| Service worker → extension (on the port) | `{t: "cancel"}` when the page stops reading (a live event stream closing) |

The same answers as Chrome's offscreen page gives its service worker.

### What keeps it safe

| Risk | What stops it |
|---|---|
| A website frames the bridge to get the box's requests | Box Desktop serves the bridge with `frame-ancestors moz-extension:`. The background page takes messages only from its own frame (`event.source`) and the box's address (`event.origin`) |
| A website posts a form to the box's address | The service worker refuses any page load that isn't a `GET` (`403`). Box Desktop's own changes are `fetch` requests from its own pages, which only its pages can make. The extension adds the box's `Origin` only for those |
| Someone else with a certificate for the box's name serves a fake service worker or page | Every answer from Box Desktop's address over HTTPS must show a certificate key the box sealed (`tls_keys`, SHA-256 of the SubjectPublicKeyInfo). Checked with Firefox's `webRequest.getSecurityInfo` before the answer loads; anything else is blocked |
| The box's certificate changes while the laptop is away | The new key comes with the next WebRTC answer, sealed by the box key, so the check follows it. The home endpoint (TCP 7445) isn't checked this way: its answers are sealed by the box key anyway |
| Pages read in between | As in Chrome: everything goes over WebRTC, tied to the pinned box key. The box's address is never fetched away from home |
| Cookies | Kept by the extension, as in Chrome, never in Firefox's cookie jar |

### Why Manifest V2

Firefox's Manifest V3 background pages stop when idle, which would drop the
WebRTC connection and the bridge frame. Firefox keeps Manifest V2, with a
background page that stays (`persistent: true`), and its blocking
`webRequest` for the certificate check. The package asks for `storage`,
`webRequest` and `webRequestBlocking`; the box's address is asked for at
pairing, as in Chrome.

### Limits

| Limit | Why, and what happens |
|---|---|
| Private windows, away from home | Private windows keep no service worker. At home they work, straight to the box |
| Firefox's data for the box's address cleared (or disk space very low) | The service worker is gone. The popup says to open ZOOMRIYA once at home, which puts it back by itself |
| Firefox 140 or newer | The link key is an Ed25519 key in the browser, which older versions can't make |

### Tested

On one computer, Firefox 153 ESR, with a real box (connectd and Box
Desktop's preview), the real extension package and the real relay:

| Check | Result |
|---|---|
| Pairing with a code, with the service worker going in | **Passed**, about half a second |
| Box Desktop opened with its address unreachable ("away") | **Passed**: the page, scripts, styles, fonts, pictures, all over WebRTC |
| The WebRTC offer through the relay, with the home endpoint unreachable too | **Passed** |
| Sign-in, box health, live event stream | **Passed**; open and live after a minute, and for six minutes in the earlier test |
| A form posted as a page load | **Refused** |
| The certificate key check | Not yet run with a real certificate: the test box used plain HTTP |

## How it reaches the box

| Option | Away from home | At home |
|---|---|---|
| 1–3: ZOOMRIYA Connect | The extension sends its WebRTC offer through the relay, showing its pass. WebRTC then connects straight to the box through both routers | WebRTC finds the box on the home network |
| 4: your Cloudflare Tunnel | The same encrypted connection to the box, carried through Cloudflare. Cloudflare can't open it | WebRTC finds the box on the home network |

It's the same idea as the app: start on whatever answers, then go direct when
possible. The app uses iroh for this. Browsers don't let extensions use iroh's
direct paths, so the extension uses WebRTC, which browsers do themselves. The
box already speaks WebRTC to guests.

## How you see the box

You open ZOOMRIYA from the extension's button, and it opens in a tab. The
pages come from the box itself, through the extension's encrypted connection.
Chrome-family browsers show them from the extension's own address, Firefox
from Box Desktop's own address.

| What | Why |
|---|---|
| Pages come through the encrypted connection, which checks the box's key | Nobody in between, Cloudflare included, can read or change them |
| The extension holds no copy of the box's screens | Box Desktop and the apps change on the box, never in two places |
| The address bar shows the extension (Chrome family), or the box's name with port 7443 (Firefox) | Where each browser lets the extension answer the pages |
| ZOOMRIYA ID sign-in, as always | The extension is a way to reach the box, not a way around sign-in |

## Pairing a laptop

Pairing happens at home, on the home network, never from away. Laptops often
have no camera, so the box shows a short code instead of a QR code.

| Step | What happens |
|---|---|
| 1 | An admin picks **Add a laptop** in Box Desktop |
| 2 | The box shows a short code. It works once, for a few minutes, with a few tries |
| 3 | You type the code into the extension |
| 4 | The extension and the box swap keys over the home network. The code only approves the swap: it's never the key |
| 5 | The box pins the laptop's key and adds it to its list. The extension pins the box's key, and with ZOOMRIYA Connect gets a pass |

An admin removes a laptop the same way as a phone.

If the family switches between ZOOMRIYA Connect and their Cloudflare
Tunnel, the laptop pairs again: at home, or with a pairing code an admin makes
for it in Box Desktop and reads out, for example over the phone. That code
works from away, only for a device already on the box's list.

## The laptop's key

The extension makes its device key itself, and the browser won't let anyone
export it, not even the extension. With ZOOMRIYA Connect, it vouches for the
everyday link key, as on a phone.

This isn't as strong as a phone's security chip: the browser locks the key,
not a chip. We'll say so plainly.

| What happened to the laptop | Its key | What to do |
|---|---|---|
| The extension is updated | Kept | Nothing |
| The extension removed, or the browser profile deleted | Gone | Pair again at home |
| Lost or stolen | Locked by the browser | An admin removes it |

## Starting up and moving

The extension runs whenever the browser does. When you switch from Wi-Fi to a
hotspot, or the laptop wakes up, it reconnects by itself, waiting a little
longer between each try.

## Backups

The same rules as phones: with ZOOMRIYA Connect, backups wait for a direct
connection, unless your own relay allows them. With your Cloudflare
Tunnel, files up to 90 MB back up through Cloudflare. Bigger files wait
until you're home (see
[Cloudflare's limits](sharing-and-names.md#cloudflares-limits)).

## Updates

The extension comes signed from each browser's own add-on store, and updates
the same way. A copy from anywhere else can't update itself in.

## What it can't do

| Limit | Why |
|---|---|
| Safari, for now | Needs a Mac to build and test |
| The box's name in the address bar, in Chrome-family browsers | The tab belongs to the extension |
| A few things in Firefox | See [Firefox's limits](#limits) |
| The box's apps (not Box Desktop) | Not yet in any browser: the extension reaches Box Desktop only, for now |
| Port forwards over TCP | WebRTC can't use the box's TCP door |

## If the checks fail

If the box's pages can't run from the extension (check 1), laptops get a
small desktop app instead. It shares its code with the phone app, so, like
the phone app, it works with ZOOMRIYA Connect and with your Cloudflare
Tunnel.

| What | How |
|---|---|
| Connecting | As on phones: iroh, or through Cloudflare |
| Your browser | A setting (a PAC file) sends only the box's names through the app, which passes your browser's own HTTPS on unopened. The address bar shows the box's real name |
| Its key | In the TPM on Windows, Keychain (with the Secure Enclave where there is one) on macOS, Secret Service or the TPM on Linux |
| Installers | Signed MSI or MSIX, a notarised DMG, and deb, rpm, AppImage or Flatpak |
