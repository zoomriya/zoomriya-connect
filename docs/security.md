# Security model

Part of [ZOOMRIYA Connect](../README.md).

To report a security problem, see [SECURITY.md](../SECURITY.md). Please never
use a public issue.

The idea is simple: no single part of ZOOMRIYA Connect can read your data or
let a new device in. Not our servers, not the relay, not the network you're
on. This page goes through each part and says what someone could and couldn't
do if they took it over.

## If someone took over…

| What | They could | They couldn't |
|---|---|---|
| **Our control plane** | Stop phones finding a box that has moved to another relay. Phones that already know where the box is, and have a pass, keep connecting. See when someone looks up a box, and from where. Serve a bad guest page. Because it answers DNS for box names, it could get a certificate for your box's name and pretend to be your box to a browser. The box watches the public list of certificates and tells you if that happens | Let a new device in. Read or change your data. Pretend to be your box to the app, which checks the box's key. Send your phone to the wrong place, because the box signs its own address |
| **A relay** | See which devices connect to which box, when, from where, and how much data goes through. Slow connections down or drop them. Ignore a pass's date, or the box saying a device was removed | Read or quietly change your data, or a guest's album. Let a device into the box, which checks its own list every time. Make a pass |
| **A copied pass** | Nothing. A pass names one device's link key, and only that device can prove it has the key | Get through the relay, or into the box |
| **A link key copied from a running app** | Connect as that device until its next new pass, at most 30 days, or until an admin removes it | Make a new link key, which needs the device key in the security chip |
| **The network you're on** | Block or slow the connection | Read or change it. Connect as your phone |
| **A lost phone or laptop** | Use the box until an admin removes it | Anything after that. The box refuses it at once and tells the relay to refuse it too. Its pass runs out within 30 days anyway |
| **A leaked share link** | Show the albums in that link, until it runs out or you cancel it | Reach anything else on the box |
| **Anyone on the internet** | Reach the album viewer while a share link is live, and poke at it. Knock on the family side of the door, which turns them away | Reach the sign-in page or any app, which are never on the internet. Get past the family side without being paired. Open an album without its link. Change anything |

## The open door for guests

While a share link is live, anyone on the internet can reach the box on port
443. Browsers get only the read-only album viewer there. Your paired devices
can use the same door as a direct way in, but the box checks every device's
key and turns away anyone it hasn't paired, just as it does through the
relay. If you don't want any door open, switch sharing off and the box closes
it.

The door leads to its own port on the box, never to the port the sign-in
page uses at home, so a router that makes internet traffic look local can't
open the sign-in page either. Each time the door opens, the control plane
checks from outside that the sign-in page can't be reached through it. If it
can, the box closes the door at once and tells you.

## The relay

Every family device stays in touch with the relay, so whoever runs it learns who
connects and when. It never learns what. We'll say who runs our relays, what
they keep, and for how long. You can switch the relay off. Then, away from
home, the app reaches the box only over IPv6 or a port you've forwarded. Or
you can [run your own](sharing-and-names.md#your-own-server).

The relay is never what keeps your data safe. The encryption between your
device and the box does that, and so does each side checking the other's key.
We'll check this for the family connection and for guests' WebRTC
connections alike.

## Passes

The box signs each paired device's [pass](how-it-works.md#the-box-hands-out-passes)
with its own key, so only the box can say which devices are its own. The pass
is for the relay, to keep strangers from using it. It never lets a device
into the box: the box checks its own list on every connection. So a relay
that ignored passes would only waste its own bandwidth. It still couldn't let
anyone in.
