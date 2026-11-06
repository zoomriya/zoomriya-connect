# Sharing, names and certificates

Part of [ZOOMRIYA Connect](../README.md).

## Sharing with guests

A guest can open a shared album in a browser without installing an app,
creating an account or pairing a device. The link gives read-only access to
the albums selected by the owner.

- **The link.** `https://share.zoomriya.net/maplehouse#<token>.<box-fingerprint>`.
  The token and box fingerprint are after `#`, so the browser does not send
  them to the page's web server. The token opens only the albums selected for
  that link.
- **Read-only, always.** Guests never write: no uploads, comments or reactions. For a share token the box accepts only GET and HEAD, for those albums' views and their photos.
- **The link is a secret.** Its token is checked on every request, and
  repeated attempts are rate-limited. Wrong, expired and revoked tokens all
  receive the same response.
- **Temporary blocks.** The box's firewall briefly blocks a source that
  repeatedly sends wrong tokens or fails to connect. The block is short
  because people behind carrier-grade NAT may share a public address with
  strangers.
- **Owner's controls.** An expiry, a view limit, an optional passcode, and revoke at any time.
- **How a guest connects.**
  1. **Direct connection.** The page requests the album from the box's share
     name, such as `maplehouse-share.zoomriya.app`. That name points to the
     home's public address only while a share link is active and leads only
     to the share listener. This works when the home has an open path, even
     if the guest is using cellular data.
  2. **Browser connection (WebRTC).** If a direct connection is unavailable,
     the page and box try to connect through a WebRTC data channel. The
     control plane passes setup messages and provides STUN address discovery;
     it does not relay the album. The page checks the box fingerprint from
     the link, and the box checks the share token.
  3. **If neither route works,** the page explains that the album cannot be
     opened from that network. When creating the link, the owner is warned
     about this possibility. The phone app can offer to send the photos using
     the phone's own sharing feature instead.
- **Trade-offs to be aware of.**
  - The guest's browser runs a viewer page from `share.zoomriya.net`. If that
    site were compromised, it could serve code that copies the album being
    viewed. The access is limited to that one read-only share. Hashed assets,
    reproducible builds and the option to host the page yourself can reduce
    this risk. Serving the page is not the same as relaying the album.
  - During the WebRTC route, the browser's address bar shows
    `share.zoomriya.net`, not the box's name.
  - WebRTC may fail if both sides use strict NAT or a network blocks UDP.
  - `connectd` needs a WebRTC implementation to support this route.
- **The owner gets a connection check.** An outside test and the detected
  NAT type help explain whether guests should be able to open the link:
  "Guests can open this link", "Guests may fail on some networks", or "Guests
  can't open links from this home".
- **Export elsewhere** is an explicit owner choice and never the default.

## Names and domains

The design uses separate domains for the shop and help pages, box names, and
the services that help Connect work:

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
