# Architecture

Part of [ZOOMRIYA Connect](../README.md).

This page is for people who want to know how ZOOMRIYA Connect is put together:
what each part is, what it does, what it knows, and how the parts work
together. If you're thinking of running your own server, checking our
security claims, or just curious, start here. For a gentler tour, read
[How it works](how-it-works.md) first.

None of this is built yet. It's the design we're building towards.

![Phones and laptops with the ZOOMRIYA app reach the box with pinned keys, and connectd hands them to the box's apps. Each connection starts through the relay and moves to a direct path when one works; the relay keeps forwarding encrypted traffic only if none does. Guests open a share link in any browser with nothing installed: over port 443 or by WebRTC hole punching, with a relay as the last option for the encrypted read-only share session. The sign-in page and apps are never on the internet. The control plane only serves the boxes' address records, passes guests' signals, answers DNS and serves the guest page; it carries no family or guest data.](architecture.svg)

## The idea in one paragraph

Your data lives on the box, and it only ever travels between the box and
devices it trusts. Everything else in ZOOMRIYA Connect exists to help those two
find each other and get a connection through home routers and strict
networks. The servers in the middle, the control plane and the relay, can see
that a connection happens, but never what's inside it, and neither of them
can let a device in. Only the box decides who gets in.

## Where things run

The parts live in three places:

| Where | Parts |
|---|---|
| **Your devices** | The link, inside the ZOOMRIYA app on phones and the desktop app on laptops |
| **Your box** | connectd, the box's web server, and the album viewer for guests |
| **Servers** (ZOOMRIYA's, or your own) | The control plane, the relay, and the guest page |

The box itself doesn't need any of the servers. Everything on it keeps
running if they're all down. Only reaching it from outside, and opening it by
name in a browser, depend on them.

## The parts

### The link (on your phone or laptop)

The link is the part of the ZOOMRIYA app that talks to the box. It's built on
[iroh](https://github.com/n0-computer/iroh), which connects two devices by
their keys rather than their internet addresses, and uses QUIC, an encrypted
connection that can switch networks without dropping.

What it does:

- **Holds the device's key.** The key is made on the device and locked in its
  security chip or key store: the Secure Enclave on an iPhone, the Keystore on
  Android, the TPM or Keychain on a laptop. It never leaves the device.
- **Pins the box's key.** It learns the box's key when you pair, and refuses
  anything else that answers, even with a valid-looking certificate.
- **Holds the device's pass** and shows it to the relay.
- **Finds the box.** It asks the control plane for the box's signed address
  record, and keeps the last one it saw, in case the control plane can't be
  reached.
- **Picks the best path.** It starts on the relay, tries every direct path at
  once, and moves to the first one that works.
- **Runs a local proxy.** The app's screens, or a laptop's browser, send their
  requests to this proxy, and it carries them over the link to the box.

The link never opens your traffic. Your browser, or the app's web view, makes
its own HTTPS connection to the box's web server, checks the box's real
certificate, and sends that connection through the link unopened. So two
layers of encryption protect it: the link's, and the browser's own.

![Inside the link: your browser's encrypted connection to the box runs through the app and connectd, so neither of them can read it.](link.svg)

On laptops, the [desktop app](desktop.md) adds a setting that sends only the
box's addresses through the proxy. Everything else goes to the internet as
usual.

### connectd (on the box)

connectd is ZOOMRIYA Connect's part on the box. It's the only part that faces
outside, and it's where every decision about who gets in is made.

What it does:

- **Holds the box's key.** Made at first setup, it never leaves the box. It
  proves the box is the box, signs the box's address record, and signs
  passes.
- **Keeps the list of paired devices** and checks every connection against
  it, whichever way the connection came in.
- **Hands out passes,** renews them, and tells the relay straight away when a
  device is removed.
- **Keeps in touch with the relay,** with one small connection that's always
  open, so the box can be reached at any moment.
- **Publishes the box's address record** to the control plane: which relay
  it uses, and the addresses it can be reached at. The box signs it, so
  nobody can change it on the way.
- **Gets the box's certificate,** with help from the control plane, and
  watches the public certificate logs. It warns you if anyone else gets a
  certificate for your box's name.
- **Opens and closes the guests' door** on your router while share links are
  live, and asks the control plane to check from outside that the door
  really works.
- **Hands connections on.** Family connections go to the box's web server,
  guests' connections to the album viewer.

connectd listens on port 443. Two kinds of traffic arrive there:

- **UDP** carries iroh connections from paired devices. Anything that can't
  prove it's a paired device is turned away before it reaches anything else.
- **TCP** carries HTTPS. From your home network, it goes to the box's web
  server, as you'd expect. From the internet, it goes only to the album
  viewer, and only while a share link is live. If you've forwarded port 443
  yourself, paired devices can use TCP too, on networks that block UDP. The
  start of each connection says which one it's for.

### The box's web server and the album viewer (on the box)

The **web server** serves the box's sign-in page and all its apps. It answers
your home network, and connections that come through the link. It's never
reachable from the internet.

The **album viewer** is a separate, much smaller service, just for guests. It:

- only answers requests to view the albums in a share link (only reads, never
  changes)
- checks the link's token on every request
- slows down and briefly blocks anyone guessing tokens
- gives the same answer for a wrong, expired or cancelled link, so a guesser
  learns nothing

Keeping it separate means a mistake in the album viewer can't reach the
sign-in page or the apps.

### The relay (on a server)

The relay is a server that both sides can always reach. We run iroh's relay,
set up for ZOOMRIYA, and families can run their own.

What it does:

- **Starts every family connection.** The phone reaches the box through the
  relay first, so the app opens quickly, before any direct path is found.
- **Helps with hole punching.** It tells each side its own public address,
  and passes the "reach out now" messages between them.
- **Carries the data when nothing direct works,** still end-to-end encrypted.
- **Keeps strangers off.** It lets in only registered boxes, and devices with
  a valid pass, and connects each device only to the box that signed its
  pass. It also keeps each box's list of removed devices.
- **Keeps backups off** our relays. A family's own relay can allow them.
- **Carries guests' albums as a last resort,** through a separate guests'
  relay for WebRTC (a TURN server).

You reach the relay over HTTPS on port 443, the same way you reach a website,
which is why it works on almost any network.

What the relay sees: which devices connect to which box, when, from where, and
how much data passes. What it never sees: what's in that data.

### The control plane (on a server)

The control plane is the coordinator. It never carries family data.

What it does:

- **Keeps boxes' address records.** We use iroh's address server
  (`iroh-dns-server`) for this. Records are signed by each box, so the control
  plane can store and serve them, but not change them.
- **Answers DNS for box names** under `zoomriya.app`.
- **Helps boxes get certificates.** It puts the temporary DNS record Let's
  Encrypt asks for, using a key that works only for that box's own name.
- **Checks from outside** whether a box's door really works.
- **Passes setup messages for guests' WebRTC connections,** never the album.
- **Tells the relays which boxes are registered.** It's never told about
  devices.

What it sees: when a box's address is looked up, and from where. What it
never sees: your data, your devices, or the box's certificate key.

### The guest page (on a server)

The guest page, at `share.zoomriya.net`, is a static web page that opens a
share link in the guest's browser. It loads, reads the token and the box's
fingerprint from the part of the link after `#` (which browsers never send to
the server), and fetches the album from the box. The album never passes
through the guest page's server.

## Keys and who trusts what

| Key | Made on | Never leaves | Used for |
|---|---|---|---|
| **Device key** | Each phone or laptop | Its security chip or key store | Proving the device is the device |
| **Box key** | The box, at first setup | The box | Proving the box is the box, signing its address record, signing passes |
| **Certificate key** | The box | The box | The padlock in browsers, for the box's name, the guests' door, and your own domain |
| **DNS update key** | Given to the box at setup | The box | Changing the box's own DNS record, and nobody else's |
| **Share token** | The box, for each link | The link (after the `#`) | Opening the albums in that link |

The rules that follow from this:

- **The box trusts only its own list.** A pass, a relay, or the control plane
  can never get a device into the box.
- **Devices trust only the box's key.** A valid certificate isn't enough. A
  look-alike box with the right name but the wrong key is refused.
- **The relay trusts the box's signature.** It lets a device through because
  the box signed its pass, not because anyone else vouched for it.
- **Nobody trusts the network in between.** Everything is encrypted end to
  end.

## How it all works together

### Setting up the box

1. You pick a name at first setup, such as `maplehouse`.
2. The box makes its key, and registers with the control plane. The control
   plane tells our relays about the box.
3. The box gets its certificate. Let's Encrypt asks for proof, the control
   plane puts a temporary record in DNS, and the certificate arrives. Its key
   was made on the box.
4. The box opens its always-on connection to the relay, and publishes its
   signed address record.
5. `maplehouse.zoomriya.app` now points at the box on your home network.

### Pairing a phone

1. The box shows a QR code on its screen. It holds the box's key, how to reach
   it, and a one-time pairing secret.
2. The app scans it, makes the phone's own key, and connects to the box on
   your home network.
3. Each side checks the other: the phone checks the box's key from the QR
   code, and the box checks the one-time secret.
4. The box adds the phone to its list and gives it its first pass.

### Connecting away from home

![How your phone connects to the box, step by step: the box stays in touch with the relay; the app asks which relay the box uses, or remembers it; it shows its pass and connects through the relay at once; the relay tells each side where the other is and both reach out together; usually within seconds the direct path takes over. If nothing direct works, the relay keeps passing the data along, still encrypted.](connecting.svg)

1. The app asks the control plane for the box's address record, or uses the
   one it remembers. It checks the box's signature on it.
2. It connects to the box's relay, and shows its pass. The relay checks the
   box's signature on the pass, the end date and the removed list, and
   connects the phone to that box only.
3. The phone and the box set up an encrypted connection through the relay,
   each checking the other's key. The box checks its list. The app is now in
   use.
4. In the background, both sides try every direct path at once: the home
   network, IPv6, hole punching, the box's open door, and TCP 443 if you've
   forwarded it. The first that works takes over, and the relay drops out.
5. If the phone's pass has less than three weeks left, the box sends a new
   one over the connection.

![Family devices start on the relay and try up to five direct paths at once, with a sixth later if tests show it's needed; the first that works takes over. Guests try the box directly, then WebRTC, then the relay.](paths.svg)

### Removing a device

1. An admin removes the phone in Box Desktop.
2. The box drops it from its list. From now on, the box refuses it on every
   path.
3. The box tells the relay at once, and again each time it reconnects, so the
   relay refuses it too.
4. The box never renews its pass, which runs out within 30 days anyway.

### A guest opens a link

1. You make a share link. The box makes a token, and opens the guests' door:
   it asks your router to open port 443, or opens it over IPv6, and points
   the door's name, such as `maplehouse-share.zoomriya.app`, at your home.
   The control plane checks from outside that it works, and the box tells you
   what guests can expect.
2. The guest opens the link. The page loads from `share.zoomriya.net`.
3. The page tries, in order:
   - **the door:** straight to the box's album viewer, over HTTPS
   - **WebRTC:** the browser and the box punch through to each other, with the
     control plane passing the setup messages. The page checks the box's
     fingerprint from the link.
   - **the guests' relay:** carrying the album, encrypted, as a last resort
4. The album viewer checks the token on every request.
5. When the last link runs out, the box closes the door and removes the
   door's name.

### At home

At home, ZOOMRIYA Connect hardly takes part. `maplehouse.zoomriya.app` points
at the box on your home network, so any browser opens it directly, with a
proper padlock. Your browser still looks the name up in DNS, which our
control plane answers. The app doesn't need even that: it finds the box on
the home network by itself.

## When things go wrong

| If this fails | What still works | What stops |
|---|---|---|
| **The control plane** | Phones away from home that know where the box is, and have a pass. The app at home, which finds the box by itself | Finding a box that has moved to another relay. Opening the box's name in a browser, once saved DNS answers run out. New certificates. Guest links, which need its DNS and setup messages |
| **The relay** | Direct paths, if the phone can find one without help: at home, over IPv6, through an open door or a forwarded port | Connections that need the relay to start or carry them. If you run your own relay, you can turn on **If my relay is down, use ZOOMRIYA's** |
| **The box's internet** | Nothing from outside. Everything at home | All access from outside |
| **The router won't open a door** | Family connections, which never need the door. Guests over WebRTC or the relay | Guests coming straight in |
| **The network blocks UDP** | The relay, over HTTPS. A forwarded TCP 443 | Direct paths over UDP |

More detail, network by network, is in [Where it works](where-it-works.md).

## Running your own

You can replace our servers with your own, partly or fully:

| What you run | What it replaces | See |
|---|---|---|
| **Just a relay** | Our relay. We still find your box and answer for its name | [Just a relay](sharing-and-names.md#your-own-server) |
| **The full server** | Our relay, control plane and guest page. ZOOMRIYA's servers play no part | [Your own server](sharing-and-names.md#your-own-server) |

The box and the apps don't change. You type your server's address into Box
Desktop once, and the pairing QR code passes it on to phones and laptops.

## Ports at a glance

| Where | Port | What for |
|---|---|---|
| **The box**, from your home network | TCP 443 | The web server (HTTPS) |
| **The box**, from the internet, while a share link is live | TCP 443 | The album viewer for guests |
| | UDP 443 | Direct connections from paired devices |
| **The box**, if you forwarded port 443 | TCP 443 | Also paired devices, on networks that block UDP |
| **The box**, otherwise | Any UDP port | Hole punching, which needs nothing opened |
| **The relay** | TCP 443 (HTTPS) | Starting and carrying connections |
| **The control plane** | TCP 443 (HTTPS), and 53 (DNS, UDP and TCP) | Address records, setup messages, DNS for box names |

The box never asks your router to open anything except the guests' door,
and only while a share link is live.

## What we build ourselves

Most of the connection work is iroh's. Our own parts are:

- passes, and the relay's checks of them
- the box's address records, signed with its own key
- TCP 443 for families who forward a port, because iroh's direct paths use
  UDP only
- the guests' door, the album viewer, and the guest page
- names, DNS and certificates for boxes
- the outside check

Where the code for each part goes is in [Plan and layout](plan.md#where-the-code-goes).
