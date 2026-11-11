# Architecture

Part of [ZOOMRIYA Connect](../README.md).

How ZOOMRIYA Connect is put together: each part, what it knows, and what each
rule protects against. For a gentler tour, read [How it works](how-it-works.md)
first. None of this is built yet.

![Phones and laptops with the ZOOMRIYA app reach the box with pinned keys, and connectd hands them to the box's apps. Each connection starts through the relay and moves to a direct path when one works; the relay keeps forwarding encrypted traffic only if none does. Guests open a share link in any browser with nothing installed: over port 443 or by WebRTC hole punching, with a relay as the last option for the encrypted read-only share session. The sign-in page and apps are never on the internet. The control plane only serves the boxes' address records, passes guests' signals, answers DNS and serves the guest page; it carries no family or guest data.](architecture.svg)

## The idea

Your data lives on the box and only travels between the box and devices it
trusts. The servers in the middle can see that a connection happens, never
what's in it, and can never let a device in. Only the box decides.

| Where | Parts | Needed for |
|---|---|---|
| **Your devices** | The link, in the ZOOMRIYA app and the desktop app | Reaching the box |
| **Your box** | connectd, the web server, the album viewer | Everything. The box keeps working if every server is down |
| **Servers** (ours, or your own) | The control plane, the relay, the guest page | Reaching the box from outside, and opening it by name in a browser |

## The parts

### The link (on your phone or laptop)

Built on [iroh](https://github.com/n0-computer/iroh), which connects devices
by their keys, over QUIC, and survives switching networks.

| What it does | Why |
|---|---|
| **Holds the device's two keys** (below) | Proves the device is the device |
| **Pins the box's key** from pairing, and refuses anything else, even with a valid certificate | A look-alike box can't fool it |
| **Holds the pass** and shows it to the relay | Only paired devices use the relay |
| **Finds the box** from its signed address record, and keeps the last one | Still works when the control plane is down |
| **Picks the best path:** starts on the relay, tries every direct path at once, moves to the first that works | Opens fast, then goes direct |
| **Runs a local proxy** for the app's screens, or a laptop's browser | The box's apps work away from home |

The link never opens your traffic: the browser's own HTTPS connection to the
box passes through it unopened, so there are two layers of encryption.

![Inside the link: your browser's encrypted connection to the box runs through the app and connectd, so neither of them can read it.](link.svg)

| Device key | Where it lives | What it's for | If it's copied |
|---|---|---|---|
| **Device key** | Made in the security chip (Secure Enclave, Keystore, TPM or Keychain). Never leaves it. Usable once the phone has been unlocked after starting, not only while unlocked | The box pins it. It signs each new link key, also in the background with the phone locked | It can't be |
| **Link key** | In the app, locked by the chip while the app isn't running, with the same "after first unlock" setting. New with each pass | iroh connects with it. iroh needs an Ed25519 key in memory, which most chips can't hold | Works until the next pass (at most 30 days), or until an admin removes the device |

### connectd (on the box)

The only part that faces outside, and where every "who gets in" decision is
made.

| What it does | Why |
|---|---|
| **Holds the box's key.** Never leaves the box. Each signature says what it's for (address record or pass) | One kind of signature can't be passed off as another |
| **Keeps the list of paired devices** and checks every connection, however it came in | A pass or a server can never let a device in |
| **Hands out and renews passes,** and tells the relay at once when a device is removed | The relay turns removed devices away too |
| **Stays connected to the relay** | The box can be reached at any moment |
| **Publishes its signed address record** | Nobody can send phones to the wrong place |
| **Gets the box's certificate** and watches the public certificate logs | Warns you if anyone else gets one for your box's name |
| **Opens and closes the guests' door,** and has it checked from outside | Guests get in only while a link is live, never to the sign-in page |
| **Hands connections on:** family to the web server through a local socket, guests to the album viewer | Each side reaches only its own service |

| Port | Who reaches it | Where it goes | What it fixes |
|---|---|---|---|
| **UDP 443** | Paired devices, from anywhere | iroh. Anything else is turned away | |
| **TCP 443** | The home network only | Not connectd: the box's web server (the gateway) keeps it, as today | Nothing at home changes. The router never sends internet traffic here, and the outside check warns if someone forwards it by mistake |
| **TCP 8443** | The internet, through the guests' door (the router's 443) | The album viewer, and paired devices on networks that block UDP. Never the web server | Some routers make internet traffic look local. connectd goes by the port, which no router can change |
| **The door's own IPv6 address** | The internet, over IPv6, when the router and provider allow it | The same as 8443 | IPv6 can't change ports, so the door gets its own address instead. Many routers can't open IPv6 doors, or give the box only one address: then there's no IPv6 door, and guests use the IPv4 door, WebRTC or the relay |

### The web server and the album viewer (on the box)

The **web server** serves the sign-in page and all the apps, to your home
network and the link. It's never reachable from the internet. Connections
from the link reach it through a local socket that only connectd can use,
labelled with the device they came from (PROXY protocol v2).

| Rule | What it fixes |
|---|---|
| Never trust a connection because it comes from the box | Every connection from away would otherwise look local |
| Accept device labels only on connectd's local socket, never on TCP 443 | Any device on the Wi-Fi could otherwise send a fake label |
| ZOOMRIYA ID sign-in is always required | The label says which device, never who's signing in |
| **5 wrong passwords lock that person's sign-in for 15 minutes**, wherever they come from, even with the right password. Written in the family's activity | Guessing stays slow, and family away from home don't share one limit |
| The activity log names the device, such as *Mum's phone* | Instead of *the box* |
| Removing a device signs out its sessions | A removed phone's open session ends too |

The **album viewer** is a separate, small service for guests.

| Rule | What it fixes |
|---|---|
| Only reads the albums in a share link, never changes anything | A guest can't touch anything else |
| Checks the token on every request | A cancelled link stops at once |
| Slows down and briefly blocks anyone guessing tokens | Guessing links |
| The same answer for a wrong, expired or cancelled link | A guesser learns nothing |
| Holds no certificate key: connectd handles guests' encryption | A break-in finds no key to pretend to be your box |
| Separate from the web server | A mistake in it can't reach the sign-in page or apps |

### The relay (on a server)

iroh's relay, set up for ZOOMRIYA. Families can run their own. Reached over
HTTPS on port 443, like a website, so it works on almost any network.

| What it does | Why |
|---|---|
| **Starts every family connection** | The app opens quickly, before a direct path is found |
| **Helps hole punching:** tells each side its public address, passes "reach out now" | Direct paths through home routers |
| **Carries data when nothing direct works,** still end-to-end encrypted | Strict networks still work |
| **Lets in only registered boxes and devices with a pass,** each only to its own box | Strangers can't use it |
| **Keeps backups off** our relays | Big uploads wait for a direct path. A family's own relay can allow them |
| **Carries guests' albums as a last resort** (TURN) | Guests on strict networks |

It sees which devices connect to which box, when, from where, and how much.
Never what's inside.

| Limit, per box, on our relays | What it fixes |
|---|---|
| **Monthly volume**, set from the [soak test](testing.md) (a home uses about 3 GB). Past it, the box is slowed, never cut off | The relay can't tell a backup from browsing, so it doesn't trust the app to hold back |
| **Speed** | One box can't crowd out the others |
| **Higher limits** for ready-made boxes, with a factory token | Self-installed boxes still work, with less to abuse |
| **Box Desktop shows this month's use** | The family sees it before anything slows |
| **No limits on a family's own relay** | Their server, their rules |

| Guests' relay (TURN) rule | What it fixes |
|---|---|
| **Blocked addresses:** private, home and loopback, and our own servers | Using the relay to reach inside our network, or anyone's private network |
| **Credentials per share session,** valid for an hour. The box asks the control plane for them after checking the guest's token, and sends them to the guest in the WebRTC setup messages | Guests can't ask for credentials themselves, because the control plane never sees the token. A leaked credential is useless after an hour, and for anything but that album |
| **Per-guest speed and volume limit** | One guest using up the relay |

### The control plane (on a server)

The coordinator. It never carries family data.

| What it does | Why |
|---|---|
| **Keeps address records** (`iroh-dns-server`), signed by each box | It can serve them, never change them |
| **Answers DNS for box names** under `zoomriya.app`. One server now; a second, at another hosting company, can be turned on later | Names keep answering if one is down |
| **Puts Let's Encrypt's temporary DNS record**, with a key that works only for that box's name | Certificates without opening the router |
| **Checks from outside** that the door works and the sign-in page can't be reached through it. Only ever checks the asking box's address | Catches a door that leaks, and can't be used to scan others |
| **Passes guests' WebRTC setup messages,** never the album | Guests reach the box directly |
| **Tells relays which boxes are registered,** never about devices | It doesn't learn your devices |
| **Moves a name, or marks a box stolen,** when an admin's phone shows its admin note, signed by the box | Only the family's admins can, and the control plane still keeps no list of devices |

It sees when a box's address is looked up, and from where. Never your data,
your devices, or the certificate key.

| Registering a box | What it fixes |
|---|---|
| One name per box | Grabbing many names |
| Blocked words (bank, login, pay…) and look-alike brand names refused | Scam names with a real padlock |
| At most 10 new boxes a day per internet address. Boxes with a factory token don't count. Past it: *Too many new boxes from your internet connection today. Try again tomorrow.* | Scripts registering in bulk, without stopping honest families who share an address with many homes (CGNAT) |
| A released name waits 90 days | Taking a name someone just gave up |
| Anyone can report a name, and we can switch it off | Abuse that gets past the rules |
| A factory token in ready-made boxes, for higher relay limits. One token per box, usable for one registration only | ZOOMRIYA OS is open source, so anyone can register a box. A token copied to other PCs is refused |

### The guest page (on a server)

A static page at `share.zoomriya.net`. It reads the token and the box's
fingerprint from after the `#`, which browsers never send to the server, and
fetches the album from the box. The album never passes through it.

## Keys and who trusts what

| Key | Made on | Never leaves | Used for |
|---|---|---|---|
| **Device key** | Each phone or laptop, in its security chip | The chip | Signing link keys. The box pins it |
| **Link key** | The app, with each pass | The device | iroh's connection. Named in the pass |
| **Box key** | The box, at first setup | The box | Proving the box, signing address records and passes |
| **Certificate key** | The box | The box | The padlock: the box's name and door, or your own domain and its door. connectd and the web server hold it, never the album viewer |
| **DNS update key** | Given to the box at setup | The box | Changing the box's own DNS record only |
| **Share token** | The box, per link | The link (after the `#`) | Opening that link's albums |
| **Admin note** | The box, signed for each admin's phone | The phone | Lets that phone move the box's name or mark the box stolen. The control plane checks the box's signature on it, so it never keeps a list of devices |

| Who | Trusts | So |
|---|---|---|
| **The box** | Only its own list | A pass, relay or control plane can never let a device in |
| **Devices** | Only the box's key | A look-alike box with a valid certificate is refused |
| **The relay** | The box's signature on a pass | Nobody else can vouch for a device |
| **Nobody** | The network in between | Everything is encrypted end to end |

## How it all works together

### Setting up the box

| Step | What happens |
|---|---|
| 1 | You pick a name, such as `maplehouse` |
| 2 | The box makes its key and registers. The control plane tells our relays |
| 3 | The box gets its certificate: the control plane puts the temporary DNS record, the key stays on the box |
| 4 | The box connects to the relay and publishes its signed address record |
| 5 | `maplehouse.zoomriya.app` points at the box on your home network |

### Pairing a phone

| Step | What happens |
|---|---|
| 1 | The box shows a QR code: its key, how to reach it, a one-time secret |
| 2 | The app makes its device key in the chip and its first link key, and connects at home |
| 3 | The phone checks the box's key from the QR code. The box checks the secret and the device key's signature on the link key |
| 4 | The box lists the device key and gives a first pass for that link key. An admin's phone also gets an admin note |

### Connecting away from home

![How your phone connects to the box, step by step: the box stays in touch with the relay; the app asks which relay the box uses, or remembers it; it shows its pass and connects through the relay at once; the relay tells each side where the other is and both reach out together; usually within seconds the direct path takes over. If nothing direct works, the relay keeps passing the data along, still encrypted.](connecting.svg)

| Step | What happens |
|---|---|
| 1 | The app gets the box's address record, or uses the one it remembers, and checks the box's signature |
| 2 | It shows its pass to the relay, which checks the signature, dates and removed list, and connects it to that box only |
| 3 | Phone and box connect through the relay, each checking the other's key. The box checks its list. The app is in use |
| 4 | Both try every direct path at once: home network, IPv6, hole punching, the door, a forwarded port. The first that works takes over |
| 5 | With under three weeks left on the pass, the app makes a new link key signed by the device key, and the box sends a new pass. The old link key stops working |

![Family devices start on the relay and try up to five direct paths at once, with a sixth later if tests show it's needed; the first that works takes over. Guests try the box directly, then WebRTC, then the relay.](paths.svg)

### Removing a device

| Step | What happens |
|---|---|
| 1 | An admin removes it in Box Desktop |
| 2 | The box refuses it on every path, and signs out its sessions |
| 3 | The box tells the relay at once, and again on each reconnect |
| 4 | The pass is never renewed, and runs out within 30 days anyway |

### A guest opens a link

| Step | What happens |
|---|---|
| 1 | You make a link. The box makes a token and opens the door: the router's 443 to the box's 8443, or the door's IPv6 address. The door's name, such as `maplehouse-share.zoomriya.app`, points at your home |
| 2 | The control plane checks from outside. If the sign-in page can be reached, the door closes at once and you're told. Otherwise you're told what guests can expect |
| 3 | The guest opens the link. The page loads from `share.zoomriya.net` |
| 4 | It tries the door, then WebRTC (checking the box's fingerprint from the link), then the guests' relay |
| 5 | The album viewer checks the token on every request |
| 6 | When the last link ends, the box closes the door and removes its name |

### At home

`maplehouse.zoomriya.app` points at the box on your home network, so a browser
opens it directly, with a proper padlock.

| Way in at home | Needs DNS? | When it fails |
|---|---|---|
| Phone app, desktop app | No: they find the box on the home network | Never |
| `maplehouse.zoomriya.app` in a browser without the app | Yes | Routers and DNS filters that throw away public answers pointing inside the home (a protection against DNS rebinding) |
| `zoomriya-xxxx.local` | No, and it uses the box's own certificate | Never |

When the name is thrown away, Box Desktop suggests the desktop app or the
`.local` name, only when someone actually hits it.

## When things go wrong

| If this fails | What still works | What stops |
|---|---|---|
| **The control plane** | Phones that remember the box's address and have a pass. The app at home | Finding a box that moved relay. The name in a browser, once saved answers run out. New certificates. Guest links |
| **The relay** | Direct paths found without help: home, IPv6, the door, a forwarded port | Connections that need the relay. Own-relay families can turn on **If my relay is down, use ZOOMRIYA's** |
| **The box's internet** | Everything at home | All access from outside |
| **The router won't open a door** | Family connections. Guests over WebRTC or the relay | Guests coming straight in |
| **The network blocks UDP** | The relay, over HTTPS. A forwarded port | Direct paths over UDP |
| **The box itself** | Nothing from outside until a box is set up again | See below |

More, network by network: [Where it works](where-it-works.md).

### If the box is lost or restored

Families keep nothing for ZOOMRIYA Connect. The box keeps its key in `/data`,
never in backups. The box phrase (planned: *Recovery keys* in zoomriya-docs,
`/developers/recovery/`) is for the box and its backups only.

| What happened to the box | Its key | What the family does |
|---|---|---|
| **Off for a while** | Kept | Nothing. After 30 days away, phones renew their passes at home |
| **A restore point, or Reset the system** | Kept | Nothing |
| **Data restored from a backup, same box** | Kept | Nothing |
| **Erased, or a new box, restored from a backup** | New | Pair phones again at home. An admin's phone moves the name, with its admin note from the old box |
| **Box and every admin's phone lost** | Gone | Pick a new name |
| **Stolen** | With the thief | An admin's phone marks it stolen, with its admin note. The relay and control plane refuse its key |
| **Dies while a link is live** | | Nothing. The door's lease runs out and the router closes it |

| What happened to the phone or laptop | Its keys | What to do |
|---|---|---|
| **The app is updated** | Kept | Nothing |
| **A new phone, even restored from the old one's backup** | Not there: kept out of phone backups | Pair at home. Remove the old one in Box Desktop |
| **The app reinstalled, or the phone reset** | Gone | Pair again at home |
| **Lost or stolen** | Locked by its chip | An admin removes it |

| Rule | What it fixes |
|---|---|
| Phones never accept a new box key by themselves. They say: *Your box has a new key. If you reset or replaced it, pair again at home.* | A look-alike box would look exactly the same |
| Pairing happens only at home, at the box. Someone abroad who loses their phone waits until they're home. Later, an admin's phone may approve a new phone from away | A stolen backup or copied app can't add a device |

## Running your own

| What you run | What it replaces | See |
|---|---|---|
| **Just a relay** | Our relay. We still find your box and answer for its name | [Just a relay](sharing-and-names.md#your-own-server) |
| **The full server** | Our relay, control plane and guest page | [Your own server](sharing-and-names.md#your-own-server) |

The box and apps don't change. You type your server's address into Box
Desktop once, and the pairing QR code passes it on.

## Ports at a glance

| Where | Port | What for |
|---|---|---|
| **The box**, from your home network | TCP 443 | The web server (the gateway), not connectd |
| **The box**, from the internet, while a link is live | Router's TCP 443 → box's 8443 | The album viewer |
| | UDP 443 | Direct connections from paired devices |
| **The box**, if you forwarded a port yourself | Router's TCP 443 → box's 8443 | Also paired devices, on networks that block UDP. Box Desktop shows the port |
| **The box**, otherwise | Any UDP port | Hole punching, nothing opened |
| **The relay** | TCP 443 | Starting and carrying connections |
| **The control plane** | TCP 443, and 53 (DNS) | Address records, setup messages, DNS for box names |

The box opens nothing on your router except the guests' door, and only while
a link is live.

## Versions and updates

| What | Rule | What it fixes |
|---|---|---|
| **iroh** | One exact version, never forked. The relay checks passes through iroh's access hook; if the hook can't see a pass, the device hands it over first. Upgraded every quarter, tested with the traffic recording | iroh changes often; a fork would need redoing on every update |
| **Protocol versions** | Servers accept the current version and the one before. Older ones get *please update*, shown in plain words | Old boxes and apps fail clearly, not strangely |
| **A new minimum version** | Announced in Box Desktop and the app months ahead | Nobody is cut off by surprise |
| **The box's clock** | Passes are signed only when the clock is set from the internet, and never earlier than the software's date. The relay allows 10 minutes' difference | A wrong clock would sign passes that have already run out |
| **When the clock can't be set** | Box Desktop's bell says at once: *Your box can't get the time from the internet, so phones will stop working away from home in a few weeks. Check your internet connection.* | Otherwise passes quietly stop renewing, and phones lose the relay weeks later with no explanation |
| **Phones** | iOS 17 or later. Android with a system WebView that has proxy support, checked at start. Older phones: home only, and the app says so | Older web views can't use the link's proxy |

## Decisions and why they work

| Decision | Instead of | Why it works |
|---|---|---|
| **Build on iroh** | Nebula, or a VPN | iroh runs inside the app, so there's no VPN prompt and it works beside a work VPN. Its relay works over HTTPS, so strict networks work too |
| **Two keys per device:** the chip's key signs a link key | One key in the chip | Most chips can't hold iroh's Ed25519 key. The chip's key still decides, and a copied link key dies with the next pass |
| **Recovery by pairing again at home**, with an admin's phone moving the name | A recovery phrase for ZOOMRIYA Connect | Families keep nothing extra. It takes a minute per phone, which they'd spend on a new box anyway |
| **The box only trusts its own list** | Trusting the relay or control plane | A server that's broken into still can't let a device in |
| **Admin notes signed by the box** | Giving the control plane a list of admin phones | Only admins can move the name or mark the box stolen, and the control plane still never learns your devices |
| **The gateway keeps TCP 443; connectd takes UDP 443 and TCP 8443** | connectd in front of everything | The gateway already serves HTTPS at home, so nothing at home changes, and connectd only handles outside traffic |
| **The door on port 8443,** or its own IPv6 address | Deciding "home or internet" from the address | Routers can fake where traffic comes from, never which port it arrives on |
| **One certificate per box,** held by connectd | One certificate with every name, in the album viewer | Fits Let's Encrypt's limits, doesn't link your own domain to ZOOMRIYA, and the part facing the internet holds no key |
| **Let's Encrypt, with Google as the fallback** | One authority | Both free. Google covers Let's Encrypt's outages and weekly limit |
| **`__Host-` cookies and `Origin` checks** | The Public Suffix List | Same protection for cookies, without months of waiting on an outside list |
| **Public box names kept** | `.local` names only | Families get a real name and padlock. The apps and `.local` cover routers that block it |
| **Our own DNS server,** a second later | Cloudflare | No record limits, no cost per box, and own domains keep the one-line NS option. A second server is a setting away |
| **One NS record, or four CNAMEs** | One CNAME | One CNAME can't cover the certificate check or the door |
| **Relay limits per box,** slowed not cut off | Trusting the app to hold back backups | The relay can't see what it carries, so it limits what it can measure |
| **Rules for registering boxes** | Only real boxes may register | Self-installs stay free, and names and the relay stay hard to abuse |
| **Device labels, and a 15-minute lock after 5 wrong passwords** | Trusting connections from the box | Every person and device is counted separately, and guessing stays slow |
| **Desktop proxy carries only box names**, no per-user check | Checking each laptop user | Everyone still signs in to ZOOMRIYA ID, so a per-user check would add code without adding safety |
| **Guests' relay credentials asked for by the box** | Guests asking the control plane | The box is the only one that can check the token |
| **Guest password optional,** off by default | Always on | Links are already unguessable. A password helps only when a link leaks, and costs guests effort |
| **Two protocol versions accepted,** minimums announced early | Forcing updates | Old boxes and apps keep working long enough to update |
| **10 minutes' clock allowance** | Exact times | Small clock differences never break passes, and a big one is caught before signing |

## What we build ourselves

Most of the connection work is iroh's. Ours:

| Part | Why it's ours |
|---|---|
| Passes, and the relay's checks | iroh's relay has no passes |
| Signed address records | The box signs with its own key |
| TCP on the door's port, for families who forward a port | iroh's direct paths are UDP only |
| The guests' door, album viewer and guest page | Guests use browsers, not iroh |
| Names, DNS and certificates for boxes | |
| The outside check | |

Where the code goes: [Plan and layout](plan.md#where-the-code-goes).
