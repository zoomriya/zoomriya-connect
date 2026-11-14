# Sharing, names and certificates

Part of [ZOOMRIYA Connect](../README.md).

## Sharing with guests

You can share an album with anyone. They open the link in their browser. They
don't install anything, make an account or pair a device. They can look, and
that's all.

### The link

A link looks like this:

`https://share.zoomriya.net/maplehouse#<token>.<box-fingerprint>`

The token works like a ticket for the albums you picked. The box's
fingerprint lets the guest's browser check it's really talking to your box.
Both come after the `#`, and browsers never send that part to the website, so
our server never sees them.

### What guests can do

- **Look, never change.** No uploads, comments or likes. For a share link, the
  box only answers requests to view those albums and their photos.
- **Nothing else.** The link can't reach any other album or app.

### Keeping links safe

- The box checks the token on every request.
- Someone guessing tokens gets slowed down. A wrong, expired or cancelled link
  all get the same answer, so a guesser learns nothing.
- If one address keeps sending wrong tokens, the box blocks it for a short
  while. Only briefly, because people on mobile data often share an address
  with strangers.
- You can set an end date, a limit on views or a passcode, and cancel a link
  at any time.

### How a guest's browser reaches the album

1. **Straight to the box.** While a link is live, the box opens a door for
   guests, by itself. There's nothing to set up. The door leads only to the
   album viewer, and the page fetches the album through it. This works
   whenever your home can open a door, even if guests are on mobile data.
2. **Browser to box, with WebRTC.** If there's no door, the browser and the
   box try to reach each other directly, the way video calls do. Our control
   plane passes the setup messages, but never the album. The page checks the
   box's fingerprint from the link, and the box checks the token.
3. **Through the relay, as a last resort.** If that fails too, the relay
   carries the album, encrypted. It carries only this one read-only album,
   nothing else on the box.
4. **If nothing works,** the page says the album can't be opened from this
   network.

When you make a link, the box tells you what to expect: "Guests can open this
link", "Guests may have trouble on some networks", or "Guests can't open links
from this home".

### Things to know

- The page a guest sees comes from `share.zoomriya.net`. If someone broke into
  that site, they could change the page to copy the album being viewed, but
  only that one read-only album. Fixed file checksums, builds anyone can
  repeat, and the option to host the page yourself make this harder.
- When the album comes over WebRTC, the address bar shows
  `share.zoomriya.net`, not your box's name.
- The relay makes things a little slower. Whoever runs it can see that a guest
  connected, when, and how much, but can't see the photos.
- Sending photos anywhere else, such as another service or an email, is always
  your choice, never automatic.

## Names and domains

ZOOMRIYA uses a few domains, each for one job:

| Domain | What it's for | Who sees it |
|---|---|---|
| `zoomriya.com` | The shop and help | Everyone |
| `maplehouse.zoomriya.app` | Your box's name. You pick it. It points at the box on your home network, never at your home's address on the internet | Your family |
| `zoomriya.net` | Our servers that help find your box (`connect.`, `ns1.`, `ns2.`, the relays), and the guest page (`share.zoomriya.net`) | Guests see `share.zoomriya.net` in a link |

`zoomriya.app` holds only families' boxes. Box names are public, because every
certificate is listed in public logs, and anyone can tell that a
`zoomriya.app` name belongs to a ZOOMRIYA Box. So setup suggests a name that
doesn't say who you are: a street or a word, not your surname.

### Four ways to reach your box

![Four ways to reach the box from outside. Options 1 to 3 are ZOOMRIYA Connect, with ZOOMRIYA's servers or your own in the middle; devices go direct when they can, and the middle sees who and when, never what. Option 4 is your own Cloudflare Tunnel: phones use the ZOOMRIYA app and laptops the ZOOMRIYA browser extension, everything goes through Cloudflare, and Cloudflare can see shared guest albums. In every option nothing is set up on the router.](options.svg)

| Option | The box's name | Who looks after its DNS | In the middle |
|---|---|---|---|
| 1. A ZOOMRIYA name (the default) | `maplehouse.zoomriya.app` | ZOOMRIYA | ZOOMRIYA's relay |
| 2. Your own domain, through ZOOMRIYA | `home.smith.ca`, handed to ZOOMRIYA with one NS record at your DNS provider ([see below](#your-own-domain-through-zoomriya)) | ZOOMRIYA | ZOOMRIYA's relay |
| 3. [Your own server](#your-own-server) | `home.smith.ca` | Your server | Your relay |
| 4. [Your own Cloudflare Tunnel](#your-own-cloudflare-tunnel) | `home.smith.ca` | Cloudflare, on your account | Cloudflare |

| Option | Suits families who | Upkeep |
|---|---|---|
| 1 | Want it to just work | None |
| 2 | Want their own name | A few DNS records |
| 3 | Don't want ZOOMRIYA involved at all | A server of their own: updates, certificates, the bill |
| 4 | Don't want ZOOMRIYA involved, and don't want to look after a server | A Cloudflare account and a domain |

Your own domain is just a domain. `home.smith.ca` doesn't tell anyone there's a
ZOOMRIYA Box behind it.

### Your own domain, through ZOOMRIYA

You add records at your domain provider, wherever you bought `smith.ca`. No
server of your own is needed: they point at names ZOOMRIYA already answers.
Box Desktop shows the exact lines to copy, checks them, and says which one is
wrong if something's missing.

**One NS record**, if your provider allows it (most do):

| Record | Points at | Covers |
|---|---|---|
| `home.smith.ca` NS | `ns1.zoomriya.net` and `ns2.zoomriya.net` | Everything under `home.smith.ca`: the box's name, the guests' door, and both certificate checks |

**Four CNAME records**, only if your provider doesn't allow NS records for
part of a domain:

| Record | Points at | What for |
|---|---|---|
| `home.smith.ca` CNAME | `maplehouse.zoomriya.app` | The box's name |
| `_acme-challenge.home.smith.ca` CNAME | `_acme-challenge.maplehouse.zoomriya.app` | The certificate check for the box's name |
| `share.home.smith.ca` CNAME | `maplehouse-share.zoomriya.app` | The guests' door |
| `_acme-challenge.share.home.smith.ca` CNAME | `_acme-challenge.maplehouse-share.zoomriya.app` | The certificate check for the door |

A single CNAME on `home.smith.ca` isn't enough: it doesn't cover the names
under it, which the certificate and the door need.

On your own domain, the box always gets a name under it, like
`home.smith.ca`. Never `smith.ca` itself: that stays for your website, your
email and everything else.

The box's name never points at your home's address on the internet. Away from
home, the app doesn't use the name to find the box, so pointing it at your
home would only tell people where you live. The only things that ever point
at your home are the guests' door, which the box opens by itself while a share
link is live, and, if you switch the relay off, the box's address record (see
[the relay](security.md#the-relay)).

### Dynamic DNS from another company

Reaching the box through a dynamic DNS name from another company, without
ZOOMRIYA Connect, isn't supported. It would mean forwarding a port on your
router and putting the box's sign-in page and apps on the open internet. The
box doesn't allow that. It only lets in your home network, and the only thing
it ever opens to the internet is the read-only album viewer. And if your
provider shares your address (CGNAT), dynamic DNS can't help anyway.

If you'd rather not depend on ZOOMRIYA, [run your own server](#your-own-server)
instead. Everything keeps working, and nothing is exposed. Or, if you don't
want to look after a server either, use
[your own Cloudflare Tunnel](#your-own-cloudflare-tunnel): nothing on your
router is opened, and the sign-in page and apps still stay off the internet.

### Your own server

You can run ZOOMRIYA Connect's server side yourself, on your own server, under
your own domain. One small server is plenty for a home. ZOOMRIYA's servers then
play no part at all: not in finding your box, not in carrying its traffic, not
in its name.

**You type one address.** In Box Desktop, under ZOOMRIYA Connect, you enter
your server's address, for example `https://connect.smith.ca`. The box reads
everything else from a small file on that server
(`https://connect.smith.ca/.well-known/zoomriya-connect.json`). Phones and
laptops don't need to type anything, because the pairing QR code includes the
address.

**Each name has one job.** For the domain `smith.ca`:

| Name | Points at | Used by | What for |
|---|---|---|---|
| `smith.ca` | Whatever you like | You, not ZOOMRIYA | Your website, your email, everything else. Never the box |
| `connect.smith.ca` | Your server | The box and the apps, in the background | Lists your other names, knows where your box is, tells your relay which box is yours, and checks from outside whether the box can be reached |
| `relay.smith.ca` | Your server | The box and the apps, in the background | Your relay: tried alongside the direct ways, helps with hole punching, and carries the connection when nothing direct works |
| `share.smith.ca` | Your server | Guests | The page a share link opens, and the guests' relay |
| `home.smith.ca` | Your box, on your home network | Your family, in a browser at home | Your box. `https://home.smith.ca` opens it with a proper padlock |

**Why so many names?**

- `share.smith.ca` runs in guests' browsers. Browsers keep different names
  apart, so it must never share a name with anything your family signs in to.
- `home.smith.ca` points at your home, while the others point at your server.
- `connect.` and `relay.` could be one name. Keeping them apart means you can
  move the relay to another server, or add a second one, without touching the
  box or anyone's phone.

**DNS.** `connect.`, `relay.` and `share.` are normal records at your DNS
provider. For `home.smith.ca`, you add one NS record, which hands that name to
your server. Your server can then point the name at your box, open and close
the guests' door under it, and handle the certificate check, the same way
ZOOMRIYA does for `zoomriya.app`. That one record is all you add.

**Your relay, your rules.** On your own relay, ZOOMRIYA doesn't limit anything.
There's no cap on how much goes through it. Any limits are yours to set, and
so is the bill from whoever hosts your server.

Your own relay also gets a switch in Box Desktop, under ZOOMRIYA Connect:
**Back up over the relay**. It's off at first, so backups wait for a direct
connection, as they do on ZOOMRIYA's relays. Turn it on, and phones back up
new photos and videos through your relay too, even when nothing direct works.

**Keeping your relay safe.** No limits from us doesn't mean no care. The box
looks out for you:

- **It keeps strangers out.** Now and then, the box tries your relay with a
  device it hasn't paired. If the relay lets it in, anyone on the internet
  could use your relay, and you'd pay for it. Box Desktop warns you ("Your
  relay lets anyone in"), and the box stops using the relay until it's fixed.
- **It shows the numbers.** Box Desktop shows how much went through your relay
  this month, for browsing and for backups.
- **You can set a limit,** if you want one. For example, "Pause backups over
  the relay after 50 GB a month". It's off unless you turn it on.
- **It tells you when something's wrong.** The bell in Box Desktop tells you
  when your relay can't be reached, when its certificate is about to run out,
  or when it needs updating.
- **A spare, if you want one.** A second switch, **If my relay is down, use
  ZOOMRIYA's**, keeps the app working while your server is down. It's off at
  first, because some families run their own server so that ZOOMRIYA is never
  involved. While ZOOMRIYA's relay stands in, our usual rules apply: backups
  wait for a direct connection.

A home uses little: about 3 GB a month for browsing, chat and the rest, and
roughly 5 to 15 GB more with backups over the relay. A small server includes
far more than that. Check what your host does when you go over its allowance:
some charge, some slow the server down, some switch it off.

**Just a relay.** If you want your traffic on your own server but don't want
to look after DNS, keep a ZOOMRIYA name and run only `relay.smith.ca`.
ZOOMRIYA still finds your box and answers for its name. Your relay checks
your devices' [passes](how-it-works.md#the-box-hands-out-passes) by itself,
so it never asks us who's allowed in. Your connections go through your relay,
with no limits from us.

**How your relay knows your box.** When you set the relay up, it shows a
one-time code. You type it into Box Desktop, under ZOOMRIYA Connect, and the
box introduces itself to the relay. From then on, the relay lets in your box,
and devices with a pass from it, and nobody else. The code works once, and
only for a few minutes. With the full server, your own `connect.` does this
for you, the way ZOOMRIYA's control plane does for our relays.

### Your own Cloudflare Tunnel

For families who want neither ZOOMRIYA's servers nor a server of their own to
look after. You bring a domain and your own Cloudflare account. The box keeps
an outgoing connection open to Cloudflare, called a Cloudflare Tunnel, so
nothing on your router is opened. ZOOMRIYA's servers play no part.

This isn't ZOOMRIYA Connect: there's no relay, no hole punching and no
control plane. Cloudflare sits in the middle every time.

![Option 4, your own Cloudflare Tunnel. Cloudflare, on your own account, has two names: home.smith.ca goes to the family door on the box, and share.home.smith.ca goes to the album viewer only. The ZOOMRIYA app on a phone, or the ZOOMRIYA browser extension on a laptop, seals everything for the box; a guest's browser needs nothing. The router opens nothing, because the box dials out. In the box, cloudflared hands family requests to connectd, which checks the device's key, then to the web server, which is never on the internet; guest requests go only to the album viewer. Cloudflare can see shared guest albums, but can't open the family's sealed requests.](cloudflare-tunnel.svg)

**How it compares**

| | ZOOMRIYA Connect (options 1–3) | Your own Cloudflare Tunnel (option 4) |
|---|---|---|
| In the middle | A relay, only when nothing direct works | Cloudflare, always |
| What the middle sees | Who connects, when, how much | Which name, when, how much, **and the photos in shared albums** |
| Laptops away from home | The ZOOMRIYA [browser extension](laptops.md) | The same extension. It supports both |
| Phones away from home | The ZOOMRIYA app | The same ZOOMRIYA app. It supports both |
| Family data | Encrypted between the device and the box | Sealed for the box by the app or the extension. Cloudflare can't open it |
| Guests | Direct, WebRTC, then the relay | Always through Cloudflare |
| The router | Nothing to set up. The box opens the guests' door by itself while a link is live | Nothing opened at all |
| Upkeep | None, or your own server | Your Cloudflare account and domain |
| Cost | Ours, or your server's | Between you and Cloudflare |

**You trust Cloudflare instead of us.** Before you switch it on, Box Desktop
says plainly what Cloudflare can see: which name is used, when and how much,
and the photos in any album you share.

**Names.** For the domain `smith.ca`:

| Name | Points at | Used by | What for |
|---|---|---|---|
| `smith.ca` | Whatever you like | You, not ZOOMRIYA | Your website, your email, everything else. Never the box |
| `home.smith.ca` | Cloudflare, then the tunnel to the family door on the box | The ZOOMRIYA app and the browser extension | Family access away from home. Cloudflare only ever passes on sealed traffic |
| `share.home.smith.ca` | Cloudflare, then the tunnel to the album viewer only | Guests | Share links. Never the sign-in page or the apps |

The two names go to two different places on the box, and browsers keep
different names apart, as with ZOOMRIYA Connect.

**Setting it up.** In Box Desktop, under ZOOMRIYA Connect, you choose **Use my
own Cloudflare Tunnel** and connect your Cloudflare account and domain. The box
keeps only a key limited to its own tunnel and names, and you can withdraw it
from Cloudflare at any time.

**The ZOOMRIYA app.** The same app works with ZOOMRIYA Connect and with your
own Cloudflare Tunnel. Only the way it reaches the box changes:

| | ZOOMRIYA Connect | Your own Cloudflare Tunnel |
|---|---|---|
| Pairing | At home, scanning the QR code | The same |
| The phone's key | In its security chip | The same |
| Away from home | iroh: direct, or through the relay | The same encrypted connection to the box, carried through Cloudflare. Cloudflare can't open it |
| The app's screens | From the box, through the link | The same |
| Backups | Wait for a direct connection, or until you're home | Files up to 90 MB go through Cloudflare. Bigger files wait until you're home |
| At home | Straight to the box | The same |

**Switching options.** If you switch between ZOOMRIYA Connect and your own
Cloudflare Tunnel, every phone and laptop pairs again:

| Way | How |
|---|---|
| At home | As usual: the QR code on a phone, the short code on a laptop |
| With a pairing code from an admin | An admin makes a code for that device in Box Desktop and reads it out to them, for example over the phone. It works from away, once, for a limited time, and only for a device already on the box's list |

**The browser extension.** Laptops use the same
[browser extension](laptops.md) as with ZOOMRIYA Connect. Only the way it
reaches the box changes.

| What the extension does | Why |
|---|---|
| Makes its own key, which the browser won't let anyone export | Nothing outside the extension, Cloudflare included, ever holds it |
| Shows the box's own pages only as they came through its encrypted connection | Cloudflare can't change what runs on the laptop |
| Carries the same encrypted connection to the box as the app, and checks the box's key | Cloudflare passes it on but can't read or change it |
| At home, reaches the box on the home network | Nothing goes through Cloudflare at home |
| Comes signed from the browser's own add-on store | A fake copy can't update itself in |

**Pairing.** Pairing happens at home, on the home network, and never goes
through the tunnel. Phones scan the QR code with the ZOOMRIYA app, as always.
Laptops have no camera, so the extension uses a short code instead.

![Pairing a laptop, then using it away. At home: an admin picks Add a laptop in Box Desktop; the box shows a short code; the family member types it into the extension; the extension makes its key and swaps keys with the box, approved by the code. Away from home: the extension seals a request for the box and sends it to home.smith.ca; Cloudflare passes it through the tunnel; the box checks the laptop's key, refuses replays, opens it and seals the answer; Cloudflare passes the answer back; the extension checks the box's key and opens it.](cloudflare-pairing.svg)

Pairing a laptop:

| Step | What happens |
|---|---|
| 1 | An admin picks **Add a laptop** in Box Desktop |
| 2 | The box shows a short code. It works once, for a few minutes, with a few tries |
| 3 | You type the code into the extension |
| 4 | The extension and the box swap keys over the home network. The code only approves the swap: it's never the key |
| 5 | The box pins the laptop's key and adds it to its list. The extension pins the box's key |

You still sign in with ZOOMRIYA ID, as always. An admin removes a laptop in
Box Desktop the same way as a phone, and the box refuses it at once.

**Guests.** A share link works the same for guests: any browser, nothing to
install, one album, read only. It always goes through Cloudflare, and Cloudflare
can see the photos in it. When you make a link, Box Desktop reminds you.

**Things to know**

- Cloudflare's limits, terms and any costs are between you and Cloudflare.
  What we know is below.
- If Cloudflare is down, or your account is, nothing reaches the box from
  outside. Everything at home keeps working.
- You can move to ZOOMRIYA Connect, or back, at any time. Each phone and
  laptop then pairs again, at home or with a pairing code from an admin (see
  [Switching options](#your-own-cloudflare-tunnel)).

#### Cloudflare's limits

What Cloudflare's own pages said when we checked, in November 2026. They can
change, so we'll check again before release.

| What | Free plan | Paid plans | Where it's from |
|---|---|---|---|
| Price per month | $0 | Pro $20 (yearly) or $25; Business $200 (yearly) or $250 | [Plans](https://www.cloudflare.com/plans/network-cdn.md) |
| Bandwidth through a tunnel | No limit stated | No limit stated | [Account limits](https://developers.cloudflare.com/cloudflare-one/account-limits/), [Tunnel](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/) |
| Largest single upload | 100 MB | Pro 100 MB, Business 200 MB, Enterprise up to 5 GB. Bigger gets error 413 | [Request limits](https://developers.cloudflare.com/workers/platform/limits/) |
| Tunnels per account | 1,000 | 1,000 | [Account limits](https://developers.cloudflare.com/cloudflare-one/account-limits/) |
| Video and many large photos | The CDN terms say they need a paid Cloudflare product (Stream, Images, R2), except on Enterprise. Cloudflare can limit an account that doesn't | The same, except Enterprise | [CDN terms](https://www.cloudflare.com/service-specific-terms-application-services/), [2023 change](https://blog.cloudflare.com/updated-tos/) |

**What this means.** Cloudflare states no bandwidth limit, on any plan. Two
things still matter:

- **Big files back up at home.** One upload can't be over 100 MB on the free
  plan, and a long phone video is bigger. So with your own Cloudflare Tunnel,
  photos and videos up to 90 MB back up through Cloudflare from anywhere.
  That leaves room for our encryption, which adds a little to each file.
  Bigger files wait on the phone or laptop until you're home, and then go
  straight to the box over the home network.
- **Playing videos is allowed by size.** The 100 MB limit is for uploads only.
  Cloudflare's terms on video are for its CDN, and its pages don't say whether
  tunnel traffic counts. Family videos travel inside our encrypted connection,
  so Cloudflare sees only how much, not that it's video. Guest albums with
  many videos are the case most likely to draw Cloudflare's attention. We'll
  ask Cloudflare before release.

## Certificates

The padlock in your browser comes from a certificate. Here's how the box gets
one:

- Each box has one certificate, from Let's Encrypt, for its name and its
  guests' door, such as `maplehouse.zoomriya.app` and
  `maplehouse-share.zoomriya.app`.
- If you use your own domain, the box's one certificate is for that instead:
  `home.smith.ca` and `share.home.smith.ca`. Your ZOOMRIYA name isn't in it,
  so the certificate doesn't tell anyone there's a ZOOMRIYA Box behind your
  domain.
- If Let's Encrypt can't issue one, the box gets it from Google Trust
  Services instead. The padlock looks the same.
- To prove the box owns those names, our control plane puts a temporary
  record in DNS, and removes it afterwards. Nothing on your router needs
  opening.
- Each box can change only its own record, with a key that works for that
  name alone. No box holds a key to anyone else's DNS.
- The certificate's secret key is made on the box and never leaves it. Our
  control plane never has it.
- With [your own Cloudflare Tunnel](#your-own-cloudflare-tunnel), Cloudflare
  gives `home.smith.ca` and `share.home.smith.ca` their padlock. The app and the
  extension don't rely on it: they check the box's own key, pinned at pairing.
