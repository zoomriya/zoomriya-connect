# Where it works

Part of [ZOOMRIYA Direct+](../README.md).

Short answer: at home, always. Away from home, from nearly any network, at
full speed where a direct path exists, and through the box's relay over Tor
where none does. The design's estimates are in [Direct+ §4](direct-plus.md#4-reach);
these are the cases.

## By the home's internet

| Home | Away, phone or laptop | Box Desktop says |
|---|---|---|
| Its own IPv4 address, router opens a port (UPnP) | ✅ Direct, full speed | Reached directly from anywhere |
| IPv6, router lets the box in | ✅ Direct over IPv6 from networks with IPv6; punched or relayed from others | Reached directly over IPv6 |
| Two routers in a row (the provider's modem and the family's) | ⚠️ Punched, or through the relay | Remote access works; steps: bridge mode |
| An address shared with other homes (CGNAT), no IPv6 | ⚠️ Punched, or through the relay | Remote access works; steps: ask the provider |

Box Desktop › Control Panel › Remote access shows the home's case and only
the steps that fit: turn on IPv6, allow the box in the router's IPv6
firewall, turn on UPnP, put the modem in bridge mode, or send the provider a
ready-made message.

## By the network the device is on

| Network | |
|---|---|
| Mobile data, home Wi-Fi of friends, hotels | ✅ Direct where possible, else through the relay |
| Networks that block Tor | ⚠️ With bridges: the laptop app's `bridges.txt`, or the public Snowflake bridges when `snowflake-client` is installed |
| Networks that block UDP | ⚠️ Through the relay (Tor runs over TCP) |
| Networks that allow only approved websites | ❌ Nothing gets through; mobile data is the answer |

## Through the relay

| | |
|---|---|
| First connection | About 10 to 15 seconds through Tor on a laptop (measured); the laptop app keeps the connection up, so this is paid once, at start |
| Moving to direct | iroh keeps punching and moves over without dropping anything; under 2 seconds when a path exists |
| What the apps do meanwhile | Send less: thumbnails first; videos and big uploads wait |

## Not tested yet

| Case | Check |
|---|---|
| A laptop on another network reaching the box through its DHT record | [Direct+ §16](direct-plus.md#16-prototype-checks) check 13 |
| A home with no direct path at all | Phase 3 |
| Bridges on a network that blocks Tor | Phase 4 |
| Phones | Phase 6 |
