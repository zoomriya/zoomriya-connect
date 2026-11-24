# How it works

Part of [ZOOMRIYA Connect](../README.md), the Direct+ design.

This page follows a phone or laptop from pairing to seeing the family's
photos, away from home. There is no ZOOMRIYA server anywhere in it. The full
design, with every decision, is in [Direct+](direct-plus.md).

| Word | Meaning |
|---|---|
| The box | The family's ZOOMRIYA Box, at home |
| connectd | The program on the box that lets paired devices in |
| The app | ZOOMRIYA Connect: built into the ZOOMRIYA app on a phone, or its own small app on a laptop |
| iroh | The connection library: encrypted QUIC between two keys, over any path |

## 1. Pairing, once, at home

| Step | Phone | Laptop |
|---|---|---|
| 1 | An admin opens Box Desktop › Control Panel › Devices › **Pair a phone** | An admin chooses **Add a laptop** |
| 2 | The box shows a QR code, for ten minutes | The box shows a short code, such as `K7P4-QXWM`, for ten minutes |
| 3 | The phone scans it on the home network | The laptop app finds the box on the home network and asks for the code |
| 4 | The phone proves it holds the code's secret | The laptop proves it knows the code without sending it (SPAKE2); three tries |
| 5 | The box adds the device to **its own list**, with the device's keys | The same |
| 6 | The box gives the app what it needs from away: its addresses, the secret behind its record in the DHT, its onion address | The same |

No internet is needed, no account is made, and nothing is sent to ZOOMRIYA.

## 2. Opening the app away from home

The app tries every way at once; the first to answer carries the data.

| Way | What it is | When it works |
|---|---|---|
| Remembered addresses | Where the box was last time | Most days, when nothing changed |
| The box's record in the DHT | The box publishes its addresses, sealed and signed, in the BitTorrent DHT; only paired devices can find and read it | When the home's address changed |
| Direct over IPv6 | The box's own lasting IPv6 address | Homes and networks with IPv6 |
| A port the router opened | The box asks the router itself (UPnP, NAT-PMP, PCP) | Homes with their own IPv4 address |
| The box's own relay, through Tor | The box runs an iroh relay behind a Tor onion service | Always, from anywhere: the last resort |

Through the relay, iroh keeps punching for a direct path and moves the
connection to it without dropping anything. On this machine's tests, Tor
took 10 to 14 seconds and the move to direct under 2 seconds.

## 3. Who gets in

| Check | Where |
|---|---|
| The device's link key is on the box's list | Every connection, every stream, every path |
| The relay lets in only the box and paired devices | The relay, on each connection |
| Removing a device in Box Desktop | Cuts its connections at once |
| Remote access turned off in Box Desktop | Only the home network; the record and the relay stop at once |

There are no passes, tokens or clocks to trust: the box's list decides.

## 4. Using the apps

| Device | How the apps open |
|---|---|
| Phone | Inside the ZOOMRIYA app |
| Laptop | In the laptop's own browser: the apps at `http://zoomriya.localhost:7480`, Box Desktop at `http://zoomriyabox.localhost:7480`. The laptop app carries each request to the box, encrypted |

When the connection runs through the relay, the apps send less: Photos shows
thumbnails first, videos and big uploads wait for a direct path, and Chat
shows messages at once and sends them in the background. Nothing on screen
says why.

## 5. Guests

Guests have only a browser. With the family's own domain, they open
`https://share.home.<domain>:8443` and see the album viewer, nothing else.
Without one, guest links work at home, and the family sends the photos to
guests elsewhere. See [Sharing and names](sharing-and-names.md).
