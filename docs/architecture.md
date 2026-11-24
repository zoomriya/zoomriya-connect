# Architecture

Part of [ZOOMRIYA Connect](../README.md), the Direct+ design.

How the code is put together, what each part knows, and what it trusts. For
a gentler tour, read [How it works](how-it-works.md) first; for the reasons
behind each choice, [Direct+](direct-plus.md).

## The parts

| Part | Runs on | Job |
|---|---|---|
| `connectd/` | The box | Pairing, the device list, the link (iroh), the box's relay and onion service, the DHT record, the router's port, the family's own name, the guests' port, Box Desktop's API |
| `crates/keys` | Everywhere | Box and device keys; SPAKE2 for laptop pairing; the sealed DHT record |
| `crates/proto` | Everywhere | What the apps and connectd say to each other |
| `crates/link` | The apps | Finding the box (home network, DHT), pairing, the connection and its path |
| `crates/tor` | The box and the apps | Tor through Arti: the box's onion services, the apps' proxy to the box's relay, bridges |
| `laptop/` | Laptops | The laptop app's core and command line: the browser server on `*.localhost:7480` |
| `laptop/app` | Laptops | The tray app (Tauri) |
| `crates/ffi`, `crates/punch` | — | The phone app's bindings and extra punching (later) |

## connectd, module by module

| Module | Job |
|---|---|
| `daemon.rs` | Starts everything; holds the shared state (`Ctx`) |
| `devices.rs` | The device list (`devices.json`): pairing codes, laptops' SPAKE2 steps, removing |
| `link.rs` | The iroh endpoint on the box's port: pairing (`zoomriya/pair/1`), the link (`zoomriya/link/1`), control and web streams to the box's own web servers |
| `relay.rs` | iroh's relay on 127.0.0.1, letting in the box and paired devices only |
| `reach.rs`, `net.rs` | The router's port (portmapper), the outside address, double NAT, the box's lasting IPv6 addresses; the DHT record, published for today and tomorrow |
| `away.rs` | Remote access away from home, on or off |
| `names.rs`, `dns_update.rs` | The family's own name: RFC 2136 updates with a TSIG key, Let's Encrypt through the DNS check, the certificate logs |
| `guests.rs` | The guests' port (TCP 8443) on the router, checked from outside through Tor; `guests.json` for Photos |
| `api.rs` | Box Desktop's API on a Unix socket (group `zoomriya-connect-api`) |

### Its API

| Route | What |
|---|---|
| `GET /v1/status` | Devices, connections, pairing, how the box is reached, Tor, remote access, the port |
| `GET /v1/devices`, `DELETE /v1/devices/{id}`, `POST /v1/devices/{id}/name` | The device list |
| `POST /v1/pairing`, `DELETE /v1/pairing` | Pairing codes |
| `PUT /v1/away` | Remote access on or off |
| `GET`, `PUT`, `DELETE /v1/names` | The family's own name (the key is never shown) |
| `GET /v1/guests` | The guests' port |

Box Desktop passes on only these routes, for a signed-in admin; removing a
device and changing the own name ask for the password again
(zoomriya-os `packages/box-desktop/src/connect.go`).

### Its files

| File (`/data/zoomriya-connect/`) | What | Who reads it |
|---|---|---|
| `box.key` | The box key | connectd |
| `devices.json` | Paired devices and their keys | connectd |
| `record.key` | The secret behind the DHT record | connectd, and every paired device |
| `away.json`, `names.json` | Remote access; the own name and its DNS key | connectd |
| `tor/`, `tor-cache/` | Tor's state (the onion service's key) and cache | connectd |
| `acme.json`, `certificates.json` | The Let's Encrypt account; the certificates it asked for | connectd |
| `certificate/connect.pem` | The own name's certificate and key | The OS agent, which serves it |
| `/run/zoomriya-link/guests.json` | The guests' port's address, once it works | The apps' gateway (group `zoomriya-link`) |

## On the wire

| What | How |
|---|---|
| Device ↔ box | iroh (QUIC, TLS 1.3), keyed to the box key and the device's link key; ALPN `zoomriya/link/1` |
| Streams | One control stream (hello, the box's addresses), one stream per web request, named by target (`apps`, `desktop`) |
| To the box's web servers | Plain HTTP on Unix sockets in `/run/zoomriya-link/` (group `zoomriya-link`) |
| The DHT record | BEP 44 mutable item; key changes daily from `record.key`; ChaCha20-Poly1305 sealed; at most 8 addresses |
| Through Tor | iroh's relay protocol over HTTP to `<onion>:80`, through the app's CONNECT proxy, which reaches only that address |
| On the home network | mDNS `_zoomriya-link._udp.local.`, TXT `key=<box key>` |

## What each part knows

| Part | Knows | Does not know |
|---|---|---|
| The box | Its devices and their keys | Anything about other homes |
| A paired device | The box's key, addresses, record secret, onion address | Other devices' keys |
| The DHT | Sealed records under keys that change daily | Which box, which family, the addresses |
| Tor relays | That someone reaches an onion service | Who, and what is said (iroh's encryption inside Tor's) |
| ZOOMRIYA | Nothing | Everything: there is no ZOOMRIYA server |
