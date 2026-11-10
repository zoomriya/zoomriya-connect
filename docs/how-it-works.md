# How it works

Part of [ZOOMRIYA Connect](../README.md).

This page follows a connection from the moment you open the app to the moment
you see your photos. Most of it isn't built yet. This is the plan.

## Your phone and the box

### Pairing

You pair a phone once, at home. The box shows a QR code on its screen. You
scan it with the ZOOMRIYA app.

Now the phone knows the box's key, and the box knows the phone's. Each one
checks the other's key every time they connect. So a phone the box doesn't
know can't get in, and nobody can pretend to be your box.

The phone keeps its key in its secure chip: the Secure Enclave on an iPhone,
the Keystore on Android. The key never leaves the phone.

### Connecting

The phone, the laptop app and the box all use
[iroh](https://github.com/n0-computer/iroh). iroh finds a device by its key,
not by its internet address.

1. **The box stays in touch with the relay.** It keeps a small connection open
   to our relay all the time.
2. **You open the app.** The phone looks up which relay the box uses and
   connects to the box through it. This is about as quick as opening a web
   page.
3. **They try to go direct.** While you're already using the app, the relay
   tells each side where the other one is. Then both try to reach each other
   at the same moment. This is called hole punching. Each router sees a
   connection going out, so it lets the reply come in.
4. **The direct path takes over.** As soon as one works, your data goes
   straight between your phone and the box. The relay drops out. This usually
   takes a few seconds.
5. **If nothing direct works,** the relay keeps passing your data along. It's
   slower, but it works. It's still encrypted, so the relay can't read it.

In step 3, they try:
- the home network, if you're at home
- IPv6, if both ends have it
- ordinary hole punching
- the box's open port, if it has one (while a share link is live, or if you
  forwarded a port yourself)

Some strict routers beat ordinary hole punching. If our
[tests](testing.md) show it's worth it, we'll add cleverer tricks later.

### What gets in

Only paired devices. The box ignores everything else and doesn't even reply,
so someone scanning the internet can't tell it's there.

A connection reaches the box's apps and nothing else on your home network.
It isn't a VPN. Your phone won't ask for VPN permission, and it works
alongside a work VPN.

### Only our relays

The app and the box use only relays run by ZOOMRIYA, or by your family if you
[run your own](sharing-and-names.md#your-own-server). iroh's public relays are
switched off.

Our relays let in only boxes and their paired devices. For each one, the
relay asks our control plane. Each time we upgrade iroh, a test records the
network traffic to make sure nothing goes anywhere else.

### Backups wait

Browsing photos, chat, the tracker and the rest all work over the relay.
Backing up new photos and videos waits until the connection is direct, or
until you're home. This keeps big uploads off the relay.

If you [run your own relay](sharing-and-names.md#your-own-server), you get a
switch, **Back up over the relay**, to let backups use it too.

## Guests and share links

A share link opens one album in any browser. Read only, nothing to install,
no account.

While a link is live, the box asks your router to open port 443 (using UPnP,
NAT-PMP or PCP), or opens it over IPv6. That door leads only to the album
viewer, never to the sign-in page or the apps. When the last link expires,
the box closes the door.

If the router won't open a door, the guest's browser and the box try to reach
each other with WebRTC. That's what browsers use for video calls. If that
fails too, the relay carries the album, encrypted. For guests, the relay is
the last resort.

The box tries IPv6 first. If your home has IPv6, guests who also have it come
straight in. This works even when your provider shares one IPv4 address
between many homes (that's called CGNAT).

More in [Sharing with guests](sharing-and-names.md#sharing-with-guests).

## The box's name

Each box has a name, such as `maplehouse.zoomriya.app`. The name points at the
box on your home network, never at your home's public address.

At home, your browser opens the box by its name, with a proper padlock. Away
from home, the app doesn't use the name. It looks up the box's signed address
record instead, which says which relay the box uses.

The padlock certificate comes from Let's Encrypt. Its private key is made on
the box and never leaves it. To prove the box owns its name, our control plane
puts a temporary record in DNS. Nothing on your router needs opening for this.

You can use your own domain instead, such as `home.smith.ca`. See
[names and domains](sharing-and-names.md#names-and-domains).

## Who carries what

| Part | What it carries |
|---|---|
| **The direct path** | Your data, almost always |
| **The relay** | The first moments of each connection. Your data only when nothing direct works. It's encrypted, so the relay can't read it |
| **The control plane** | Which relay each box uses, and setup messages for guests. Never your data |
| **The guest page** (`share.zoomriya.net`) | The viewer page only. The album itself comes from the box |
| **DNS** | Names and addresses only |

![What carries what. DNS and the control plane only help devices find each other. Family connections start on the relay and switch to a direct path when one works. Guests get the viewer page from share.zoomriya.net and the album from the box.](data-flow.svg)

![Inside the link: your browser's encrypted connection to the box runs through the app and connectd, so neither of them can read it.](link.svg)

### The order of attempts

![Family devices start on the relay and try six direct paths at once; the first that works takes over. Guests try the box directly, then WebRTC, then the relay.](paths.svg)

## The parts

| Part | Where it runs | What it does |
|---|---|---|
| **The link** | Inside the ZOOMRIYA app on phones (Android, iOS) and in the desktop app (Linux, Windows, macOS) | Connects to the box and checks its key. The app's screens, or a laptop's browser, reach the box's apps through it |
| **connectd** | The box | Lets in paired devices and nobody else. Checks share links. Opens and closes the door on the router for guests. Publishes the box's address record. Warns you if anyone else gets a certificate for the box's name |
| **The relay** | A server run by ZOOMRIYA, or by your family | Starts each connection and helps with hole punching. Carries data only when nothing direct works. Lets in only boxes and their paired devices |
| **The control plane** | A server run by ZOOMRIYA, or by your family | Keeps each box's address record, answers DNS for box names, helps get certificates, checks from outside whether a box can be reached, and passes setup messages for guests |
