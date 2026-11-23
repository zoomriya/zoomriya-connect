# Sharing, names and certificates

Part of [ZOOMRIYA Direct+](../README.md).

## Sharing with guests

Guests open a link in their browser: no app, no account, no pairing. They
see the albums shared with them in Photos' album viewer, and nothing else.

| Home | Guest link |
|---|---|
| With the family's own name, the router opened the guest port | ✅ `https://share.home.<domain>:8443/photos/view/<code>`, from anywhere |
| The same over IPv6 | ✅ For guests with IPv6, once the router's IPv6 firewall lets TCP 8443 in |
| Without an own name, or the router refused the port | Works at home; for guests elsewhere, the family sends the photos |

| The guest port | |
|---|---|
| Where | TCP 8443, the same inside and out, so the name's IPv4 and IPv6 addresses share it |
| What answers | The apps' gateway's guest-only site: `/photos/view/…`, its files, and Photos' guest API; every other address is "Not here" |
| Opened by | connectd, asking the router (UPnP) while the family has its own name and remote access is on; closed when either goes |
| Checked | By the box, from outside through Tor, as a stranger: the viewer answers, a family page does not. Box Desktop shows the result |
| Links | Photos makes them through the port once the box has checked it (`/zoomriya/guests.json`) |

Links for Tor Browser are not offered for now ([Direct+ §17](direct-plus.md#17-open-questions)).

## The family's own name

Optional. With a domain of their own, the family gets `home.<domain>` with a
real padlock in any browser at home, and `share.home.<domain>` for guests.

| What the family gives, in Box Desktop › Remote access › Your own name | |
|---|---|
| The domain | For example `smith.ca` |
| The DNS server that takes updates, the zone, a key name and a key (TSIG, HMAC-SHA256 or stronger) | From their DNS provider; Box Desktop recommends a key limited to these names |
| Or acme-dns for the certificate's check | Its server, subdomain, username and password; the family points the two `_acme-challenge` names at it once |

Only the standards are used: RFC 2136 dynamic updates and acme-dns. No DNS
provider's own API.

| What the box does | How often |
|---|---|
| Points `home.` and `share.home.` at addresses that reach the box: the lasting IPv6 addresses (AAAA), and the IPv4 address only when the router opened a port and there is one router (A); TTL 60 s | When they change |
| Gets the certificate for both names from Let's Encrypt, proved through the DNS check; the key is made on the box | When missing or under 30 days left; after a failure, 6 hours later |
| Shows the CAA record that limits certificates to the box's Let's Encrypt account | Once it has an account |
| Reads the public certificate logs for certificates it did not ask for, and warns in Box Desktop | Daily |

The certificate goes to `certificate/connect.pem`; ZOOMRIYA OS's agent checks
it and the gateway serves it beside the box's own (zoomriya-os
docs/design.md §8.1).

## Without the app

`zoomriya-xxxx.local` still opens the apps at home in any browser, after the
browser trusts the box's own authority once. It is for devices that cannot
install the app; the apps use the paths above.
