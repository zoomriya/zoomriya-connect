# Laptops: the browser extension

Part of [ZOOMRIYA Connect](../README.md).

On a laptop, the ZOOMRIYA browser extension lets you use the box from
anywhere, in every option: with ZOOMRIYA Connect, and with
[your own Cloudflare Tunnel](sharing-and-names.md#your-own-cloudflare-tunnel).
There's nothing else to install, and no admin rights are needed. It's planned
for Chrome, Edge and Firefox, on Linux, Windows and macOS.

Phones use the ZOOMRIYA app, as before.

> **This depends on three checks.** Before we build it, we check the three
> things below. If the first one fails, laptops get a small desktop app
> instead (see [If the checks fail](#if-the-checks-fail)).

| Check | What we need to see |
|---|---|
| 1. The box's pages in the extension | Box Desktop and the box's apps run from the extension's own address: sign-in, cookies, the box's `Origin` checks, and live connections (WebSockets, which need a small adapter) |
| 2. Connections from the extension | The extension can reach our relay with its pass, and set up WebRTC through it. In Chrome, WebRTC needs a hidden extension page, because the extension's background worker can't use it |
| 3. Direct connections | In the [network lab](testing.md#stage-2-the-network-lab), WebRTC reaches the box directly about as often as iroh does |

## How it reaches the box

| Option | Away from home | At home |
|---|---|---|
| 1–3: ZOOMRIYA Connect | Through the relay at once, showing its pass. Meanwhile, the extension and the box try WebRTC through the relay, and a direct path takes over when one works | WebRTC finds the box on the home network |
| 4: your own Cloudflare Tunnel | The same encrypted connection to the box, carried through Cloudflare. Cloudflare can't open it | WebRTC finds the box on the home network |

It's the same idea as the app: start on whatever answers, then go direct when
possible. The app uses iroh for this. Browsers don't let extensions use iroh's
direct paths, so the extension uses WebRTC, which browsers do themselves. The
box already speaks WebRTC to guests.

## How you see the box

You open ZOOMRIYA from the extension's button, and it opens in a tab. The
pages come from the box itself, through the extension's encrypted connection,
and the extension shows them from its own address.

| What | Why |
|---|---|
| Pages come through the encrypted connection, which checks the box's key | Nobody in between, Cloudflare included, can read or change them |
| The extension holds no copy of the box's screens | Box Desktop and the apps change on the box, never in two places |
| The address bar shows the extension, not the box's name | The tab belongs to the extension. A small banner shows the box's name |
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

If the family switches between ZOOMRIYA Connect and their own Cloudflare
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
connection, unless your own relay allows them. With your own Cloudflare
Tunnel, files up to 90 MB back up through Cloudflare. Bigger files wait
until you're home (see
[Cloudflare's limits](sharing-and-names.md#cloudflares-limits)).

## Updates

The extension comes signed from each browser's own add-on store, and updates
the same way. A copy from anywhere else can't update itself in.

## What it can't do

| Limit | Why |
|---|---|
| Safari | Not planned. Mac users use Chrome, Edge or Firefox |
| The box's name in the address bar | The tab belongs to the extension |
| Port forwards over TCP | WebRTC can't use the box's TCP door. On networks that block UDP, the extension uses the relay |

## If the checks fail

If the box's pages can't run from the extension (check 1), laptops get a
small desktop app instead. It shares its code with the phone app, so, like
the phone app, it works with ZOOMRIYA Connect and with your own Cloudflare
Tunnel.

| What | How |
|---|---|
| Connecting | As on phones: iroh, or through Cloudflare |
| Your browser | A setting (a PAC file) sends only the box's names through the app, which passes your browser's own HTTPS on unopened. The address bar shows the box's real name |
| Its key | In the TPM on Windows, Keychain (with the Secure Enclave where there is one) on macOS, Secret Service or the TPM on Linux |
| Installers | Signed MSI or MSIX, a notarised DMG, and deb, rpm, AppImage or Flatpak |
