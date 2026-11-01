# Sharing, names and certificates

Part of [ZOOMRIYA Connect](../README.md).

## Sharing with guests

A guest never installs anything and has no account or paired key, so a shared link is a plain browser visit, read-only.

- **The link.** `https://share.zoomriya.net/maplehouse#<token>.<box-fingerprint>`. Everything after `#` stays in the browser and never reaches any server. The token opens the albums its link lists, read-only, and nothing else on the box.
- **Read-only, always.** Guests never write: no uploads, comments or reactions. For a share token the box accepts only GET and HEAD, for those albums' views and their photos.
- **One secret.** The token in the link is the only secret. It is checked on every request, attempts are rate-limited, and a wrong, expired or revoked token gets the same answer.
- **Short blocks.** A source that keeps sending wrong tokens or failing handshakes is blocked by the box's firewall for a short time, before any code runs. Short on purpose: guests behind carrier-grade NAT share addresses with strangers.
- **Owner's controls.** An expiry, a view limit, an optional passcode, and revoke at any time.
- **Order of attempts.**
  1. **Direct.** The page fetches the album from the box's share name (such as `maplehouse-share.zoomriya.app`), which points at the home's public address only while a share link is live and leads only to the share listener. This works when the home has an open path, from any guest network, cellular included.
  2. **Browser hole punching (WebRTC).** If the direct try fails, the page and the box hole-punch through a WebRTC data channel. The control plane passes signaling messages and runs STUN, and nothing else: no TURN, so no relay. The page checks the box's fingerprint from the link, so the control plane can't pose as the box, and the box checks the token.
  3. **Neither works.** The page says the album can't be opened from this network right now. The owner was already told when making the link (below), and the app offers to send the photos instead, through the phone's own sharing.
- **Costs to accept.**
  - The guest runs the viewer page served from `share.zoomriya.net`. A compromised host could serve code that steals the shared album. The damage is limited to that one read-only album. Reduce the risk with hashed assets, reproducible builds and an option to host the page yourself. Serving a static page is not relaying traffic.
  - The address bar shows `share.zoomriya.net`, not the box name, on the WebRTC route.
  - The WebRTC route has the same limit as phones: it fails when both ends are behind strict NAT, or UDP is blocked.
  - connectd needs a WebRTC stack for this.
- **Checked when the link is made.** The outside test and the NAT type tell the owner: "Guests can open this link", "Guests may fail on some networks", or "Guests can't open links from this home".
- **Export elsewhere** is an explicit owner choice and never the default.

## Names and domains

Three domains, one job each:

| Domain | Think of it as | Who sees it |
|---|---|---|
| `zoomriya.com` | The shop and help desk | Everyone |
| `maplehouse.zoomriya.app` | Your home's address, a name the family picks; it always points at the box's address on the home network | The family |
| `maplehouse-share.zoomriya.app` | The door for guests' photos, pointing at the home's public address only while a share link is live | Nobody: the guest page uses it behind the scenes |
| `zoomriya.net` | The switchboard that finds your box (`connect.`, `stun1.`, `stun2.`, `ns1.`), and the guest page `share.zoomriya.net` | Nobody types it; guests see `share.zoomriya.net` in a share link |

`zoomriya.app` holds only families' boxes, never anything of ZOOMRIYA's. Box names are public (every certificate is logged publicly), so the setup suggests a name that doesn't identify the family: a street or a word, not a surname.

Three ways to name the box:

| Option | Name | DNS kept by | Hole punching |
|---|---|---|---|
| ZOOMRIYA name (default) | `maplehouse.zoomriya.app` | The control plane | Yes |
| Own domain, through ZOOMRIYA | `home.smith.ca`, a CNAME at any DNS host | The control plane | Yes |
| Own domain, on its own | `home.smith.ca` | The box, through the family's DNS | No: advanced homes with a permanent port forward or IPv6 rule only |

On its own domain, the box still runs Connect (the pinned link, the desktop app, the share view) but has no control plane, so there is no hole punching and no guest WebRTC. For independence from ZOOMRIYA with everything working, run your own control plane instead.

## Certificates

- Let's Encrypt issues the box one certificate, for its name, its share name and any custom domain.
- To check the box owns those names, Let's Encrypt looks up a temporary DNS record (a DNS challenge) that the control plane publishes for the box and deletes afterwards. No inbound port is needed. The control plane never holds the certificate or its key.
- Each box can change only its own challenge record, with a token that works for that name alone. No shared DNS credentials sit on any box.
- The certificate's private key is made on the box and never leaves it.
