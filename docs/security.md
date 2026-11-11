# Security model

Part of [ZOOMRIYA Connect](../README.md).

To report a vulnerability, see [SECURITY.md](../SECURITY.md). Please never use
a public issue.

The idea is simple: no single part of ZOOMRIYA Connect can read your data or
let a new device in. Not our servers, not the relay, not the network you're
on. This page goes through each part and says what someone could and couldn't
do if they took it over.

## If someone took over…

| What | They could | They couldn't |
|---|---|---|
| **Our control plane** | Stop new connections, though devices already holding a pass keep using the relay. See when a box's address is looked up, and from where. Serve a bad guest page. Because it answers DNS for box names, it could get a certificate for your box's name and pretend to be your box to a browser. The box watches the public certificate logs and tells you if that happens | Let a new device in. Read or change your data. Pretend to be your box to the app, which checks the box's key. Send your phone to the wrong place, because the box signs its own address record |
| **A relay** | See which devices connect to which box, when, from where, and how much data goes through. Slow connections down or drop them. Ignore a pass's expiry, or the box saying a device was removed | Read or quietly change your data, or a guest's album. Let a device in: the box checks its own list of paired devices every time. Make a pass |
| **A copied pass** | Nothing. A pass names one device's key, and only that device can prove it holds the key | Get through the relay or into the box |
| **The network you're on** | Block or slow the connection | Read or change it. Connect as your phone |
| **A lost phone or laptop** | Use the box until an admin removes it | Anything after that. The box refuses it at once and tells the relay to as well; its pass runs out within 30 days in any case |
| **A leaked share link** | Show the albums in that link until it expires or you cancel it | Reach anything else on the box |
| **Anyone on the internet** | Reach the album viewer on port 443 while a share link is live, and poke at it | Reach the sign-in page or any app (they're never on the internet). Open an album without its link. Change anything |

## The open door for guests

While a share link is live, anyone on the internet can reach the box on port
443. But that door only leads to the read-only album viewer. Paired devices
don't use it. If you don't want any door open, switch sharing off and the box
closes it.

## The relay

Every family connection starts on the relay, so whoever runs it learns who
connects when. It never learns what. We'll say who runs our relays and what
they keep, and for how long. You can switch the relay off; the app then
reaches the box away from home only over IPv6 or a port you've forwarded. Or
you can [run your own](sharing-and-names.md#your-own-server).

The relay is never what keeps your data safe. The encryption between your
device and the box does that, and so does each side checking the other's key.

## Passes

The box signs each paired device's [pass](how-it-works.md#the-box-hands-out-passes)
with its own key, so the box is the only one that can say which devices are
its own. The pass is for the relay, to keep strangers from using it. It's
never what lets a device into the box: the box checks its own list of paired
devices on every connection. So a relay that ignored passes could waste its
own bandwidth, but it still couldn't let anyone in.
We'll check both for the family link and for guests' WebRTC connections.
