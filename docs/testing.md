# Testing at scale

Part of [ZOOMRIYA Connect](../README.md).

Before release, ZOOMRIYA Connect is tested with **1,000 families over two
months**. The test should show that relay-first connections work the way iroh
intends, and what our relays carry and cost. It runs in three stages that share
one description of the families:

| Stage | What runs | Time | Answers |
|---|---|---|---|
| 1. Traffic model | A simulation, no network | 60 days in about a minute | What the relay carries and costs; which assumptions matter most |
| 2. Network lab | Real iroh endpoints, relays and control plane, behind emulated routers on one machine | 60 days of family life in 3 days | Whether connections open, go direct and recover; how the relay holds up |
| 3. Two-month soak | The same lab at real speed against relays on the internet, plus real boxes and phones | 60 days | Slow faults: leaks, renewals, upgrades, address changes, real networks |

## The families

One profile drives all three stages, generated from a fixed seed so every run
can be repeated. These starting figures are guesses; step 8 of the
[plan](plan.md) replaces them with what real homes report.

**Households**
- 1,000 boxes, 2 to 6 members each (3.5 on average), and 1.5 paired devices
  per member: about 5,000 phones and laptops.

**Home networks**

| Home network | Share |
|---|---|
| Router with easy NAT | 55% |
| Router with symmetric NAT | 10% |
| Provider's CGNAT, easy | 15% |
| Provider's CGNAT, symmetric | 10% |
| Double NAT (provider gateway and own router) | 10% |

- On top of the table, 40% of homes also have working IPv6.

**Away from home**
- Mobile data (CGNAT, often symmetric, often IPv6): 60% of time away.
- Someone else's Wi-Fi: 30%.
- A network that blocks UDP (office, hotel): 10%.

**A day**
- Members are away 9 hours on weekdays and less at weekends.
- While away, each member uses:
  - 20 chat bursts
  - a tracker report every 5 minutes while moving
  - two photo-browsing sessions of 20 MB
  - a 200 MB video every fifth day
- Each member takes 15 photos (3 MB each) and one video (60 MB) a day. Their
  backups wait for a direct path.
- Each family shares 2 albums a month with 5 guests each, at 300 MB per guest.

**Events**
- The provider changes the home's public address every 1 to 14 days.
- Phones switch between Wi-Fi and mobile data 4 times a day.
- Boxes restart weekly for updates.
- A relay restarts during the run.
- iroh gets one minor upgrade mid-run.

## Stage 1: traffic model

A discrete-event simulation over 60 days. It follows each family's day and,
for each connection, the network at each end and whether it goes direct. It
reports:

- the relay's bytes per day and per month, and the busiest hour
- how many endpoints hold a relay connection at once, and how many
  connections stay on it
- the share of family bytes that went direct
- each family's relay use: the spread, and the top 1%
- the monthly cost on our relay servers

It also shows what changes the result most: 5%, 10% or 25% of connections
staying on the relay; backups allowed on the relay or not; and per-family
monthly limits of 20, 50 or 100 GB (the limit is still to be decided). It runs
in CI and is built first; its figures are replaced by the lab's as they come
in.

## Stage 2: network lab

Real ZOOMRIYA Connect endpoints on one large machine. Linux network
namespaces stand in for homes and networks:

- **Each home** is a namespace with the box's endpoint behind an nftables NAT
  of its type:
  - easy NAT: `masquerade`
  - symmetric NAT: `masquerade random,fully-random`
  - CGNAT: a second, shared NAT namespace in front of many homes
- **Away networks** are shared namespaces: a mobile network holds hundreds of
  phones behind one symmetric CGNAT, as real ones do. The UDP-blocked network
  drops all UDP.
- **The internet** is a namespace with two relays (`iroh-relay`, standing in
  for two regions), our `iroh-dns-server` and the control plane. `tc netem`
  adds 20 to 150 ms of delay and up to 2% packet loss.

**Time is compressed 20 times:** the families' days from stage 1 are replayed
so 60 days take 3 days. Only daily life is compressed. Keepalives, NAT
timeouts and reconnects run at real speed, because they are what fails in
practice. Transfers keep their real sizes. That's about 30 Mbit/s through the
relays on average, which one machine handles.

**Measured**
- time until the first byte (through the relay) and until the connection is
  direct
- connections still on the relay after 10 seconds, and connections that never
  open
- the relays' processor, memory, connections and bytes (`iroh-relay` exposes
  metrics on port 9090)
- the address-record server's queries
- the control plane's admission checks: how many and how fast
- the time to recover after a home's address changes or a relay restarts
- stalls of more than 2 seconds that a person would notice

## Stage 3: two-month soak

The lab harness runs at real speed for 60 days. The families run in
namespaces on a few load machines and reach staging relays over the real
internet. Real participants join the same staging:

- 5 to 10 boxes in team members' homes
- 20 or more phones on different mobile providers
- laptops on office and hotel networks

Two months is the minimum because some faults only show up slowly:

- memory or connection leaks
- the boxes' certificate renewals, around day 60 of each 90-day certificate
- an iroh release
- provider address changes
- server maintenance

**Setup and cost (DigitalOcean, about $230 a month, so about $460 for the
run)**

| Server | Cost a month |
|---|---|
| Two relays in two regions | $12 each |
| One control plane and address-record server | $12 |
| Four load machines with 8 processors each | about $48 each |

The traffic fits in the servers' included allowance. Prices are from
DigitalOcean's pricing as of 2026-11; check before starting.

Metrics go to a dashboard, with an alert for each pass mark below.

## Pass marks

Proposed; agree them before stage 2.

- **Opening:** at least 99.5% of connections from networks that can reach the
  relay open within 3 seconds; the 95th percentile time to the first byte is
  under 1 second.
- **Going direct:** at least 85% of family bytes, backups apart, travel
  direct. The share of families that stay on the relay is within 25% of the
  model.
- **Backups:** none is ever sent through the relay.
- **Recovery:** after a home's address changes or a relay restarts,
  connections recover within 30 seconds (95th percentile).
- **The relay:** under 50% of the processor at the busiest hour. Memory is
  flat over the 60 days (less than 10% growth). Monthly traffic and cost are
  within 25% of the model.
- **Only our servers:** a packet capture shows no traffic to iroh's public
  relays or address servers.
- **Admission:** every unpaired endpoint is refused by the relay.
- **Certificates:** every box renews its certificate.

## What it can't show

The real mix of providers and NAT types. The lab only checks behaviour under
the mix we guessed. Real figures come from step 8 of the plan: anonymous,
opt-in connection reports from the first homes.

## Layout

```
sim/
  model/   the families and the traffic model (stage 1), run in CI
  lab/     namespaces, NAT types, the driver that replays the families (stages 2 and 3)
  soak/    staging servers, load machines, the dashboard and alerts (stage 3)
```
