# Testing

Part of [ZOOMRIYA Direct+](../README.md).

This page is for the people building Direct+.

## On every change

| Command | What |
|---|---|
| `cargo test --workspace` | Unit tests, and connectd's end-to-end tests (`connectd/src/e2e.rs`): pairing a phone and a laptop with no internet, the link, web streams, removed devices cut off |
| `cargo clippy --workspace --all-targets -- -D warnings` | No warnings |
| `cargo deny check` | Licences, bans, sources and advisories (`deny.toml`, with the reasons for each exception) |
| In zoomriya-os and zoomriya-apps | `go test ./...`; `npx vitest run` and `npx svelte-check` in each web app |

## By hand, on a real network

A test connectd runs beside the real one with its own settings: another
port, data folder and API socket, and targets pointing at a local web server.

```
zoomriya-connectd run --config test/connectd.toml
curl --unix-socket test/api.sock -H 'Content-Type: application/json' \
     -d '{"laptop":true,"name":"Test"}' http://box/v1/pairing
XDG_DATA_HOME=test/laptop zoomriya-laptop pair CODE
```

| Command | Checks | Result so far (one home: IPv4 shared, IPv6 open) |
|---|---|---|
| `zoomriya-laptop check-away` | Finds the box's record in the DHT and connects using only its addresses | ✅ |
| `zoomriya-laptop check-tor` | Connects only through the box's relay over Tor, then watches for the move to a direct path | ✅ 10 to 14 s through Tor, direct 0.5 to 1.5 s later |
| `check-tor` from a laptop that is not paired | The relay turns it away | ✅ |
| `PUT /v1/away {"on": false}`, then `check-tor` | Turned away | ✅ |
| `zoomriya-laptop run` | The browser at `zoomriya.localhost:7480` | ✅ |

## Still to check

| Check | Needs |
|---|---|
| [Direct+ §16](direct-plus.md#16-prototype-checks) checks 4 and 10 to 13 | A real box, real browsers, another network |
| A home with no direct path | A network where only Tor gets through |
| Bridges | A network that blocks Tor |
| The family's own name end to end | A real domain, its DNS server's TSIG key, Let's Encrypt |
| The guest port from outside | A router that opens it |
