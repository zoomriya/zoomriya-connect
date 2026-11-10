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
| **Our control plane** | Stop new connections. See which devices connect to which box, when, and from where. Serve a bad guest page. Because it answers DNS for box names, it could get a certificate for your box's name and pretend to be your box to a browser. The box watches the public certificate logs and tells you if that happens | Let a new device in. Read or change your data. Pretend to be your box to the app, which checks the box's key. Send your phone to the wrong place, because the box signs its own address record |
| **A relay** | See which devices connect to which box, when, from where, and how much data goes through. Much of this the control plane sees anyway. Slow connections down or drop them | Read or quietly change your data, or a guest's album. Let a device in |
| **The network you're on** | Block or slow the connection | Read or change it. Connect as your phone |
| **A lost phone or laptop** | Use the box until an admin removes it | Anything after that |
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
We'll check both for the family link and for guests' WebRTC connections.
