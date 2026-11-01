# Security model

Part of [ZOOMRIYA Connect](../README.md).

To report a vulnerability, see [SECURITY.md](../SECURITY.md); never in a public issue.

## What each party can and can't do

| If this is compromised… | It can | It can't |
|---|---|---|
| The control plane | Stop new connections; see who connects when, and IP addresses. Not send a device to the wrong place: address records are signed by the box. Because it answers DNS for box names, it could get a certificate for a box's name and pose as the box **to browsers**; the box watches the certificate logs and tells the admin. It could also serve a bad guest viewer page | Trust a device, read or change a paired device's traffic, pose as the box to the phone or desktop app (they pin the box's endpoint ID) |
| A network the device is on | Block or slow it | Read or change it, connect as a paired device |
| A lost phone or laptop | Use its access until the admin revokes it | Anything after that |
| A shared link that leaks | Show the albums it lists until it expires or is revoked | Reach anything else on the box |
| The internet at large | Reach the box's port 443 while a share link is live, and probe the read-only share view | Reach the sign-in page or any app (never served to the internet), open an album without its token, or change anything |

While a share link is live, port 443 is reachable by anyone on the internet, but it serves only the read-only share view. Paired devices never need it. A home that doesn't want it turns sharing off, and the box closes what it opened.
