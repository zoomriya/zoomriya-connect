# Security model

Part of [ZOOMRIYA Connect](../README.md), the Direct+ design.

To report a security problem, see [SECURITY.md](../SECURITY.md). Please never
use a public issue.

The idea is simple: the box decides who gets in, from its own list, and
nothing between a device and the box can read what passes. There is no
ZOOMRIYA server to trust, break into, or switch off.

## What protects what

| Threat | Protection |
|---|---|
| A stranger connects to the box | Every connection and every stream is checked against the box's device list; others are closed |
| A stranger uses the box's relay | The relay lets in only the box and paired devices, each proving its key |
| Someone on the path reads or changes traffic | iroh's QUIC with TLS 1.3, keyed to both ends' keys, pinned at pairing |
| Tor relays watch | They see an onion service in use; inside Tor's encryption is iroh's |
| Someone reads the DHT | Records are sealed (ChaCha20-Poly1305) and signed with keys that change daily; without the record secret they cannot be found or read, nor linked to a box |
| A pairing code is overheard or guessed | Phone codes are long secrets in a QR code, shown on the box; laptop codes are short but never sent (SPAKE2), give three tries, and expire in ten minutes |
| A device is lost | An admin removes it in Box Desktop; it is cut off at once |
| Away access is not wanted | Box Desktop › Remote access › off: home network only, at once |
| A program on the laptop reaches the box | The laptop app listens on loopback only, answers only its two names, and needs a cookie set by a one-time link from the app |
| A web page on the laptop rebinds DNS | Only `zoomriya.localhost:7480` and `zoomriyabox.localhost:7480` are answered |
| Guests see family data | Their port (TCP 8443) serves the album viewer and its guest API only; the box checks this from outside through Tor |
| Someone gets a certificate for the family's own name | A CAA record limited to the box's account (Box Desktop shows it); the box reads the certificate logs daily and warns of certificates it did not ask for |

## Keys

| Key | Where | Leaves it |
|---|---|---|
| Box key (Ed25519) | The box | Never; devices pin its public half |
| Device's link key (Ed25519) | The device | Never |
| Device key (P-256) | The device; the keychain or secure hardware later | Never; it vouches for the link key at pairing |
| Record secret | The box and paired devices | Given at pairing, inside the encrypted pairing connection |
| Onion service key | The box (`tor/`) | Never |
| Own name's DNS key (TSIG) | The box (`names.json`, connectd only) | Never; Box Desktop never shows it again |
| Certificate key | The box | Never; the gateway reads it to serve HTTPS |

## Limits

| Limit | |
|---|---|
| Whoever controls the family's DNS | Can point the name elsewhere and get a certificate; the CAA record and the log watch help, the apps are unaffected (they pin the box key) |
| A device removed while connected through the relay | Keeps its relay connection until it drops, but the box refuses its QUIC connections, so it reaches nothing |
| Networks that allow only approved websites | Nothing gets through; mobile data is the answer |

Data on the box is unchanged by any of this; see zoomriya-os
docs/design.md for the box itself.
