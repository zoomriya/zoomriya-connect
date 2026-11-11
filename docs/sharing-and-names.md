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
| `zoomriya.net` | Our servers that help find your box (`connect.`, `ns1.`, the relays), and the guest page (`share.zoomriya.net`) | Guests see `share.zoomriya.net` in a link |

`zoomriya.app` holds only families' boxes. Box names are public, because every
certificate is listed in public logs, and anyone can tell that a
`zoomriya.app` name belongs to a ZOOMRIYA Box. So setup suggests a name that
doesn't say who you are: a street or a word, not your surname.

### Three ways to name your box

| Choice | The box's name | Who looks after its DNS | The relay |
|---|---|---|---|
| A ZOOMRIYA name (the default) | `maplehouse.zoomriya.app` | ZOOMRIYA | ZOOMRIYA's |
| Your own domain, through ZOOMRIYA | `home.smith.ca`, handed to ZOOMRIYA with one NS record at your DNS provider ([see below](#your-own-domain-through-zoomriya)) | ZOOMRIYA | ZOOMRIYA's |
| [Your own server](#your-own-server) | `home.smith.ca` | Your server | Yours |

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
| `home.smith.ca` NS | `ns1.zoomriya.net` | Everything under `home.smith.ca`: the box's name, the guests' door, and both certificate checks |

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
home would only tell people where you live. The only thing that ever points
at your home is the guests' door, which the box opens by itself while a share
link is live.

### Dynamic DNS from another company

Reaching the box through a dynamic DNS name from another company, without
ZOOMRIYA Connect, isn't supported. It would mean forwarding a port on your
router and putting the box's sign-in page and apps on the open internet. The
box doesn't allow that. It only lets in your home network, and the only thing
it ever opens to the internet is the read-only album viewer. And if your
provider shares your address (CGNAT), dynamic DNS can't help anyway.

If you'd rather not depend on ZOOMRIYA, [run your own server](#your-own-server)
instead. Everything keeps working, and nothing is exposed.

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
| `relay.smith.ca` | Your server | The box and the apps, in the background | Your relay: starts each connection, and carries it when nothing direct works |
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
