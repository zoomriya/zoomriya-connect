# Testing at scale

Part of [ZOOMRIYA Connect](../README.md).

Before release, we test ZOOMRIYA Connect with 1,000 pretend families over two
months. We want to know three things:

- Do connections open quickly and go direct, the way iroh says they should?
- What does our relay carry, and what does it cost?
- What breaks over time?

The test has three stages. All three use the same pretend families.

| Stage | What runs | How long | What it tells us |
|---|---|---|---|
| 1. Traffic model | A simulation, no real network | Two months in a minute | What the relay carries and costs, and which guesses matter most |
| 2. Network lab | Real ZOOMRIYA Connect code behind pretend routers, on one machine | Two months of family life in 3 days | Whether connections open, go direct and recover, and how the relay copes |
| 3. Two-month soak | The same lab at normal speed, against real relays on the internet, plus real boxes and phones | Two months | Slow problems: leaks, certificate renewals, upgrades, real networks |

## The pretend families

The numbers below are our best guesses. Once real homes use ZOOMRIYA Connect,
their (anonymous, opt-in) reports replace them. Every run starts from a fixed
seed, so it can be repeated exactly.

**Who they are.** 1,000 families, each with a box. 2 to 6 people per family
(3.5 on average), with 1.5 phones or laptops each: about 5,000 devices.

**Their home internet.**

| Home internet | Share |
|---|---|
| Router, easy | 55% |
| Router, strict | 10% |
| Shared address (CGNAT), easy | 15% |
| Shared address (CGNAT), strict | 10% |
| Two routers (the provider's and their own) | 10% |

40% of homes also have IPv6.

**Where they are when they're out.** On mobile data 60% of the time, on
someone else's Wi-Fi 30%, and on a network that blocks UDP (an office or
hotel) 10%.

**A typical day.** People are out 9 hours on weekdays, less at weekends. While
out, each person:
- sends about 20 bursts of chat messages
- reports their location every 5 minutes while moving
- browses photos twice (20 MB each time)
- watches a 200 MB video every fifth day

Each person also takes 15 photos (3 MB each) and one video (60 MB) a day. Those
are backed up once the connection is direct.

Each family shares 2 albums a month, with 5 guests each, at 300 MB per guest.

**Things that go wrong.** The internet provider changes the home's address
every 1 to 14 days. Phones switch between Wi-Fi and mobile data 4 times a
day. Boxes restart weekly for updates. A relay restarts during the test. iroh
gets one update halfway through.

## Stage 1: the traffic model

A simulation that follows every family, day by day, for two months. For each
person it picks where they are, which network they're on, and whether their
connection goes direct. Then it adds up what they use.

It reports:
- how much the relay carries each day and each month, and in its busiest hour
- how many devices are connected to the relay at once
- how much of the family's data went direct
- how much each family used the relay, including the heaviest 1%
- what the relay servers cost each month

It also shows what changes the answer most:
- whether 5%, 10% or 25% of connections stay on the relay
- whether backups are allowed on the relay
- a monthly relay limit per family of 20, 50 or 100 GB (not decided yet)

It's in [`sim/model`](../sim/model) and runs with every change, in CI:
`cargo run --release -p zoomriya-sim-model`.

## Stage 2: the network lab

Real ZOOMRIYA Connect code on one big machine. Linux network namespaces, which
are like small separate networks inside one computer, stand in for homes and
networks:

- **Each home** is its own namespace, with the box behind a pretend router of
  the right kind: easy, strict, or behind a provider's shared address.
- **Places people go** are shared namespaces. A pretend mobile network holds
  hundreds of phones behind one strict router, as real ones do. The pretend
  office network blocks UDP.
- **The internet** is one more namespace, holding two relays (as if in two
  regions), our address server and our control plane. We add 20 to 150 ms of
  delay and a little packet loss.

**Two months in three days.** We replay the families' days 20 times faster.
Only the daily life is sped up. Keep-alives, router timeouts and reconnects
run at normal speed, because those are what really break. Uploads and
downloads keep their real sizes.

**What we measure:**
- how long until the first data arrives, and until the connection is direct
- connections still on the relay after 10 seconds, and ones that never open
- the relays' processor, memory, connections and data (`iroh-relay` reports
  these on port 9090)
- how busy the address server and the control plane are
- how long reconnecting takes after a home's address changes or a relay
  restarts
- any pause longer than 2 seconds that a person would notice

## Stage 3: the two-month soak

The same lab, at normal speed, for 60 days. The pretend families run on a few
machines and reach our test relays over the real internet. Real people join
in too:
- 5 to 10 boxes in the team's homes
- 20 or more phones on different mobile networks
- laptops on office and hotel networks

Why two months? Some problems only show up slowly:
- memory or connections slowly piling up
- the boxes renewing their certificates (around day 60 of 90)
- an iroh update
- internet providers changing addresses
- server maintenance

**What it costs** on DigitalOcean (November 2026 prices; check before
starting):

| Server | Each, per month |
|---|---|
| Two relays, in two regions | $12 |
| One control plane and address server | $12 |
| Four machines running the pretend families (8 processors each) | about $48 |

That's about $230 a month, so about $460 for the whole test. The traffic fits
in what the servers include.

Everything goes on one dashboard, with an alert for each goal below.

## The goals

These are suggestions. We'll agree them before stage 2.

- **Quick to open:** 99.5% of connections open within 3 seconds, from any
  network that can reach the relay. Most see their first data in under a
  second.
- **Mostly direct:** at least 85% of family data, not counting backups, goes
  direct. The number of families stuck on the relay is within 25% of what the
  model predicted.
- **Backups:** none ever goes through our relay.
- **Recovers fast:** after a home's address changes or a relay restarts,
  connections are back within 30 seconds.
- **The relay copes:** under half its processor at the busiest hour. Memory
  stays flat over two months. Its traffic and cost are within 25% of the
  model.
- **Only our servers:** recorded network traffic shows nothing going to iroh's
  public servers.
- **Strangers stay out:** the relay refuses every unpaired device, every
  expired or copied pass, and every pass from a box it doesn't know. A device
  reaches only the box that made its pass.
- **Removed means removed:** a removed device is refused by the box at once,
  and by the relay within 5 seconds, or as soon as the box reconnects.
- **Passes renew:** no family device loses the relay because its pass ran
  out while it was in regular use.
- **Certificates:** every box renews its certificate.

## What this can't tell us

Which networks real families actually have. The lab only tests the mix we
guessed. Real numbers come from the first homes, through anonymous, opt-in
connection reports.

## Where the code goes

```
sim/
  model/   the pretend families and the traffic model (stage 1), runs in CI
  lab/     the namespaces, the pretend routers, and the code that plays out the families' days (stages 2 and 3)
  soak/    the test servers, the family machines, the dashboard and alerts (stage 3)
```
