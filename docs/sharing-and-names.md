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
     it does not carry album data. The page checks the box fingerprint from
     the link, and the box checks the share token.
  3. **Relay, the last option.** If direct WebRTC also fails, the browser and
     box use our relay for the encrypted data channel. The relay carries only this scoped, read-only share
     session; it does not expose the dashboard or other apps.
  4. **If no route works,** the page explains that the album cannot be
     opened from that network. When creating the link, the owner is told
     whether relay may be used and any bandwidth limits or operator.
- **Trade-offs to be aware of.**
  - The guest's browser runs a viewer page from `share.zoomriya.net`. If that
    site were compromised, it could serve code that copies the album being
    viewed. The access is limited to that one read-only share. Hashed assets,
    reproducible builds and the option to host the page yourself can reduce
    this risk. Serving the page is not the same as relaying the album.
  - During the WebRTC route, the browser's address bar shows
    `share.zoomriya.net`, not the box's name.
  - A relay adds latency and bandwidth cost. Its operator can see endpoint
    addresses, timing and traffic volume, but must not be able to read the
    end-to-end encrypted album contents.
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
| `maplehouse.zoomriya.app` | Your home's address, a name the family picks; it always points at the box's address on the home network, never the home's public address | The family |
| `maplehouse-share.zoomriya.app` | The door for guests' photos, pointing at the home's public address only while a share link is live | Nobody: the guest page uses it behind the scenes |
| `zoomriya.net` | The switchboard that finds your box (`connect.`, `stun1.`, `stun2.`, `ns1.`), and the guest page `share.zoomriya.net` | Nobody types it; guests see `share.zoomriya.net` in a share link |

`zoomriya.app` holds only families' boxes, never anything of ZOOMRIYA's. Box names are public (every certificate is logged publicly), so the setup suggests a name that doesn't identify the family: a street or a word, not a surname.

Three ways to name the box:

| Option | Name | DNS kept by | Hole punching and relay |
|---|---|---|---|
| ZOOMRIYA name (default) | `maplehouse.zoomriya.app` | ZOOMRIYA's control plane | Yes, through ZOOMRIYA's relays |
| Own domain, through ZOOMRIYA | `home.smith.ca`, a CNAME to the box's ZOOMRIYA name, set at any DNS host | ZOOMRIYA's control plane | Yes, through ZOOMRIYA's relays |
| [Own server](#your-own-server) | `home.smith.ca` | The family's own control plane | Yes, through the family's relay |

A family's own domain is just a domain: a name such as `home.smith.ca` doesn't
say there's a ZOOMRIYA box behind it. The surname advice above is for
`zoomriya.app`, where every name is a box.

On a family's own domain, the box always gets a name under it, such as
`home.smith.ca`, never the bare `smith.ca`. The bare domain stays for the
family's website, email and everything else.

The family name never points at the home's public address. Away from home,
the app reaches the box through the link, not through DNS, so a public record
would only tell anyone who reads the certificate logs where the family lives.
Only the share name points at the public address, and only while a share
link is live.

### Third-party dynamic DNS

Reaching the box through a third-party dynamic DNS name, without ZOOMRIYA
Connect, is **not supported**. It would need a router port forward and the
box's sign-in page and apps open to the whole internet. The box's firewall
admits only the home network, and nothing on the box faces the internet
except a live share link's read-only view. Dynamic DNS also can't help a home
behind its provider's shared address (CGNAT).

For independence from ZOOMRIYA with everything working, run
[your own server](#your-own-server) instead: pairing, the pinned link, hole
punching, the relay and guest links all keep working, and nothing is exposed.

### Your own server

A family, or whoever looks after their box, can run ZOOMRIYA Connect's server
side on their own server, under their own domain, instead of ZOOMRIYA's. One
small server is enough for a home. ZOOMRIYA's servers then play no part: not
in finding the box, not in carrying its traffic, not in its name.

**The family enters one address.** In Box Desktop, under ZOOMRIYA Connect,
they type their server's address, for example `https://connect.smith.ca`.
The box reads the other names from a small file at
`https://connect.smith.ca/.well-known/zoomriya-connect.json`. Phones and
laptops never type anything: the pairing QR code carries the same address.

**Each name does one job.** With the domain `smith.ca`:

| Name | Points at | Who uses it | What for |
|---|---|---|---|
| `smith.ca` | Whatever the family likes | Not ZOOMRIYA | Stays the family's own: their website, email and everything else. Never the box's name |
| `connect.smith.ca` | Their server | The box and the apps, behind the scenes | The control plane: the file listing the other names, the boxes' signed address records (`iroh-dns-server`, looked up over HTTPS), which devices the relay admits, and the outside reachability check |
| `relay.smith.ca` | Their server | The box and the apps, behind the scenes | The relay (`iroh-relay`): starts each connection, and carries it if no direct path works |
| `share.smith.ca` | Their server | Guests | The page a share link opens, and the guests' WebRTC relay (TURN) |
| `home.smith.ca` | The box's address on the home network | The family, in a browser at home | The box's own address: `https://home.smith.ca` opens Family Hub with a real certificate |
| `share.home.smith.ca` | The home's public address, only while a share link is live | Nobody types it: the guest page uses it behind the scenes | The guests' direct route to the box's read-only share view |

**Why the names are separate.**
- `share.smith.ca` runs in guests' browsers, so it must never share a web
  origin with anything the family signs in to.
- `home.smith.ca` points at the box at home, while `connect.`, `relay.` and
  `share.` point at the server on the internet.
- `connect.` and `relay.` could be one name. Keeping them apart lets the
  relay move to another server, or add a second one elsewhere, without
  changing anything on the box or the phones.

**DNS.** `connect.`, `relay.` and `share.` are ordinary records at the
family's DNS host. `home.smith.ca` is handed to their server with one NS
record. That covers `share.home.smith.ca` too, so their server can point both
names and publish the certificate check, as ZOOMRIYA's control plane does
for `zoomriya.app`.

**Only a relay.** A family that wants their traffic on their own server but
no DNS to look after can keep a ZOOMRIYA name and run only `relay.smith.ca`.
ZOOMRIYA's control plane still finds the box, answers for its name and tells
their relay which devices to admit; the connections, and anything that stays
on the relay, go through theirs.

## Certificates

- Let's Encrypt issues the box one certificate, for its name, its share name and any custom domain.
- To check the box owns those names, Let's Encrypt looks up a temporary DNS record (a DNS challenge) that the control plane publishes for the box and deletes afterwards. No inbound port is needed. The control plane never holds the certificate or its key.
- Each box can change only its own challenge record, with a token that works for that name alone. No shared DNS credentials sit on any box.
- The certificate's private key is made on the box and never leaves it.
