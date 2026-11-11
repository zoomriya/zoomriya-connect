# How it works

Part of [ZOOMRIYA Connect](../README.md).

This page follows your phone from the moment you open the app to the moment
you see your photos. None of it is built yet. This is the plan.

A few names you'll meet:

- **The box:** your ZOOMRIYA Box at home.
- **The relay:** a server of ours that passes messages between your phone and
  the box when they can't reach each other directly. Families can run their
  own.
- **The control plane:** another server of ours. It tells your phone how to
  find the box. It never carries your photos or messages.
- **connectd:** the part of ZOOMRIYA Connect that runs on the box.

## Your phone and the box

### Pairing

You pair a phone once, at home. The box shows a QR code on its screen, and
you scan it with the ZOOMRIYA app.

From then on, the phone and the box know each other's keys. A key works like
a signature that only one device can make. They check each other's key every
time they connect. So a phone the box doesn't know can't get in, and nothing
else can pretend to be your box.

The phone's key stays on the phone, locked in its security chip: the Secure
Enclave on an iPhone, the Keystore on Android. Only the ZOOMRIYA app can use
it, and it never leaves the phone. It vouches for a second, everyday key that
the connection uses, and the phone makes a new everyday key every few weeks.

### Connecting

![How your phone connects to the box, step by step: the box stays in touch with the relay and publishes a signed record naming that relay; the app gets that record, or remembers it; it tries the relay and the box's addresses it already knows at the same moment, and the first to answer carries your data; if the relay answered first, it tells each side where the other is and both reach out together; usually within seconds the direct path takes over. If nothing direct works, the relay keeps passing the data along, still encrypted.](connecting.svg)

1. **The box stays in touch with the relay.** It keeps a small connection
   open to the relay all the time. It also tells the control plane which
   relay it uses. The box signs this record, so nobody can send phones to the
   wrong place. Like iroh, the record never holds your home's internet
   address, unless you switch the relay off (see
   [the relay](security.md#the-relay)).
2. **You open the app.** The phone gets the box's record from the control
   plane. It remembers the record, so it can still find the box if the
   control plane is down.
3. **It tries everything at once.** The phone tries the relay, and at the
   same moment every address of the box it already knows: found on your home
   network, or remembered from the last connection, when the box told it.
   Whichever answers first carries your data. At home, or when both ends have
   IPv6, that's usually the direct way, and you're done. An address that has
   changed just doesn't answer, so nothing waits on it.
4. **They go direct.** If the relay answered first, you're already using the
   app through it. Meanwhile the relay tells the phone and the box where the
   other one is. Both reach out at the same moment, so each router sees a
   connection going out and lets the reply come in. This is called hole
   punching, and it needs the relay to pass the messages.
5. **The direct path takes over.** Usually within a few seconds, your data
   goes straight between the phone and the box, and the relay drops out.
6. **If nothing direct works,** the relay keeps passing your data along. It's
   slower, but it works. Everything is encrypted, so the relay can't read it.

The direct ways it tries:

- your home network, if you're at home
- IPv6, if both ends have it
- ordinary hole punching
- the box's open door, if it has one: the
  [guests' door](#guests-and-share-links) while a share link is live, or a
  port you've forwarded yourself
- TCP port 443, if you've forwarded it, for networks that block UDP (the kind
  of traffic hole punching uses). iroh doesn't do this one, so we add it
  ourselves

Some strict routers stop ordinary hole punching. If our [tests](testing.md)
show it's worth it, we'll add cleverer tricks later.

All of this is built on [iroh](https://github.com/n0-computer/iroh), an
open-source library. iroh finds a device by its key, not by its internet
address. It also keeps measuring every path while you use it, and moves to a
faster one when there is one.

### Seeing how it's going

Box Desktop shows, for each phone and laptop, whether it's going direct or
through the relay, how quickly it connected, and how fast it is. It also shows
when a device couldn't connect or was cut off, and why. So if one
phone always ends up on the relay, you can see it, and see why. These numbers
stay on your box. We only get them if you agree to send anonymous reports,
which never say whose box or which device.

### The box hands out passes

When you pair a phone, the box also gives it a pass. It's a short note that
says "this phone belongs to this box, until such-and-such a date", signed by
the box. Only your box can make passes for itself. Not us, and not the relay.

![How passes work, in four steps. 1: at home, you pair your phone, and the box gives it a pass signed by the box. 2: away from home, the phone shows its pass to the relay, which checks it by itself and lets the phone through to that box only; the box checks its own list and lets it in. 3: a stranger, or a copied pass, is turned away. 4: when you remove a lost phone, the box tells the relay straight away, and both turn it away. Passes last 30 days and renew by themselves.](passes.svg)

Away from home, the phone shows its pass to the relay. The relay checks the
box's signature by itself, then lets the phone through, but only to that
box. The box checks the phone against its own list of paired devices, as it
does every time. A pass gets a phone through the relay, never into the box.

Because the relay checks passes by itself:

- **Our control plane knows less.** It's never asked about your devices, so
  it doesn't learn which ones are yours or when they connect. The relay still
  sees which devices connect to your box, because it passes their traffic
  along.
- **Things keep working when the control plane is down.** The relay checks
  passes on its own, and your phone remembers where the box was last time.
  The box signs that note, so an old copy can't be faked. Only a box that has
  moved to another relay can't be found until the control plane is back.
- **Your own relay only needs your box.** See
  [your own server](sharing-and-names.md#your-own-server).

**Renewing a pass.** A pass lasts 30 days and renews itself, without you
noticing:

1. Your phone connects to the box in any way: at home, directly, or through
   the relay.
2. The box checks that the phone is still on its list.
3. If the pass has less than three weeks left, the box sends a new one, good
   for another 30 days.

The relay plays no part in this. It never makes passes.

The app also reaches the box in the background now and then, for example to
back up new photos when you get home. So passes renew even if you rarely open
the app. Phones limit what apps can do in the background, iPhones most of
all, so our [tests](testing.md) check this on real phones. If passes run out
too often, we'll make them last longer. That's safe, because removing a phone
works at once, whatever its pass says.

**Removing a phone.** When an admin removes a phone, the box refuses it
straight away and tells the relay to refuse it too. If the relay can't be
reached at that moment, the box tells it as soon as it can. And the pass runs
out within 30 days anyway.

**A phone that's been away a long time.** If a phone doesn't reach the box
for more than 30 days, its pass runs out and the relay won't let it through.
It can still connect at home, or over a direct path, because there the box
only checks its own list. Then it gets a new pass, and the relay works again.

### What gets in

Only your paired devices. The relay lets through only devices with a pass,
and only to the box that made the pass. Someone who finds the box's address
some other way still can't get in: the box refuses any device it hasn't
paired.

A connection reaches the box's apps and nothing else on your home network.
It isn't a VPN. Your phone won't ask for VPN permission, and it works
alongside a work VPN.

### Only our relays

The app and the box use only relays run by ZOOMRIYA, or by your family if you
[run your own](sharing-and-names.md#your-own-server). iroh's public relays are
switched off.

Our relays let in only ZOOMRIYA Boxes, and devices with a pass from one. When
a box is set up, the control plane tells the relays about it. That's all the
relays learn from us. Each time we update iroh, a test records the network
traffic to make sure nothing goes anywhere else.

### Backups wait

Looking at photos, chat, the tracker and everything else work through the
relay. Backing up new photos and videos waits until the connection is
direct, or until you're home. This keeps big uploads off the relay.

If you [run your own relay](sharing-and-names.md#your-own-server), a switch
called **Back up over the relay** lets backups use it too.

## Guests and share links

A share link opens one album in any browser. Guests can only look. They don't
install anything or make an account.

While a link is live, the box asks your router to open a door on port 443,
the port every secure website uses. Routers let devices ask for this in a few
standard ways (UPnP, NAT-PMP or PCP). If your home has IPv6, the box opens
the door there too. The door has two sides:

- **Guests' browsers** reach only the album viewer through it. Never the
  sign-in page, and never the apps.
- **Your own paired devices** can use it as one more direct way in. The box
  turns away anyone it hasn't paired, as always.

When the last link runs out, the box closes the door.

If the router won't open a door, the guest's browser and the box try to reach
each other with WebRTC, which is what browsers use for video calls. If that
fails too, the relay carries the album, encrypted. For guests, the relay is
the last resort.

The box tries IPv6 first. When your home and the guest both have IPv6, the
guest comes straight in. This works even when your provider shares one
internet address between many homes (that's called CGNAT).

More in [Sharing with guests](sharing-and-names.md#sharing-with-guests).

## The box's name

Each box has a name, such as `maplehouse.zoomriya.app`. The name points at the
box on your home network, never at your home's address on the internet.

At home, your browser opens the box by its name, with a proper padlock. Away
from home, the app doesn't use the name. It asks the control plane where the
box is instead.

The padlock comes from a certificate, issued by Let's Encrypt. Its secret key
is made on the box and never leaves it. To prove the box owns its name, our
control plane puts a temporary record in DNS, the internet's address book.
Nothing on your router needs opening for this.

You can use your own domain instead, such as `home.smith.ca`. See
[names and domains](sharing-and-names.md#names-and-domains).

### Opening the box at home

| Device | What to open | Works on every router? |
|---|---|---|
| A phone | The ZOOMRIYA app | Yes |
| A laptop with the [desktop app](desktop.md) | `maplehouse.zoomriya.app` in your browser | Yes |
| A computer without the app | `maplehouse.zoomriya.app` | On most routers. If it won't open, use the next line |
| A computer without the app, if the name won't open | `zoomriya-xxxx.local`, the box's home name. Box Desktop shows yours | Yes |

**Why the name might not open without the app.** Your browser asks the
internet's address book (DNS) where `maplehouse.zoomriya.app` is, and the
answer is the box's address inside your home. Some routers, and ad or DNS
filters, throw that kind of answer away on purpose, because a known attack
uses it to reach devices inside homes. Your router is protecting you, but the
name then can't be found.

| Way in | Why it isn't affected |
|---|---|
| The phone app and the desktop app | They find the box on your home network by themselves, without asking DNS |
| `zoomriya-xxxx.local` | Computers find `.local` names on the home network by themselves. It uses the box's own certificate, which you trust once (*Open the apps securely* in the user guide) |

If you'd like the name to work on that computer too, install the desktop app.
Or allow `zoomriya.app` in your router's settings: Box Desktop shows how for
common routers.

## Who carries what

| Part | What it carries |
|---|---|
| **The direct path** | Your data, almost always |
| **The relay** | The first moments of a connection, when it answers before a direct path. Your data only when nothing direct works, and it's encrypted, so the relay can't read it |
| **The control plane** | Where each box is, and setup messages for guests. Never your data |
| **The guest page** (`share.zoomriya.net`) | The page a share link opens. The album itself comes from the box |
| **DNS** | Names and addresses only |

![What carries what. DNS and the control plane only help devices find each other. Family devices try direct paths and the relay together, and a direct path takes over when one works. Guests get the page from share.zoomriya.net and the album from the box.](data-flow.svg)

Inside the connection, your browser talks to the box's web server in its own
encrypted connection. The app and connectd only pass it along, so neither of
them can read it.

![Inside the link: your browser's encrypted connection to the box runs through the app and connectd, so neither of them can read it.](link.svg)

### The order of attempts

![Family devices try the box's known addresses and the relay at the same moment, and the first to answer carries data; hole punching runs through the relay, and a direct path takes over when one works. Guests try the box directly, then WebRTC, then the relay.](paths.svg)

## The parts

For the full picture, with every part, the keys and what happens when one
fails, see [Architecture](architecture.md).

| Part | Where it runs | What it does |
|---|---|---|
| **The link** | Inside the ZOOMRIYA app on phones (Android, iOS) and the desktop app (Linux, Windows, macOS) | Connects to the box and checks the box's key. The app's screens, or a laptop's browser, reach the box's apps through it |
| **connectd** | The box | Lets in paired devices and nobody else. Hands out and renews their passes, and tells the relay when one is removed. Checks share links. Opens and closes the guests' door. Tells the control plane where the box is. Warns you if anyone else gets a certificate for the box's name |
| **The relay** | A server run by ZOOMRIYA, or by your family | Is tried alongside the direct ways, and passes the messages for hole punching. Carries data only when nothing direct works. Lets in only boxes, and devices with a pass, and only to their own box |
| **The control plane** | A server run by ZOOMRIYA, or by your family | Knows where each box is, answers DNS for box names, helps get certificates, checks from outside whether a box can be reached, and passes setup messages for guests |
