# Testing at scale

Part of [ZOOMRIYA Connect](../README.md).

This page is for the people building ZOOMRIYA Connect.

Before release, we test ZOOMRIYA Connect with 1,000 pretend families over two
months. We want to know three things:

- Do connections open quickly and go direct, the way iroh says they should?
- What does our relay carry, and what does it cost?
- What breaks over time?

The test has three stages. All three use the same pretend families. Alongside
them, [benchmarks](#benchmarks-on-every-change) run on every change, and keep
running after release.

| Stage | What runs | How long | What it tells us |
|---|---|---|---|
| 1. Traffic model | A simulation, no real network | Two months in a minute | What the relay carries and costs, and which guesses matter most |
| 2. Network lab | Real ZOOMRIYA Connect code behind pretend routers, on one machine | Two months of family life in 3 days | Whether connections open, go direct and recover, and how the relay copes |
| 3. Two-month soak | The same lab at normal speed, against real relays on the internet, plus real boxes and phones | Two months | Slow problems: leaks, certificate renewals, upgrades, real networks |

## The pretend families

The numbers below are our best guesses. Once real homes use ZOOMRIYA Connect,
their reports (anonymous, and only if they agree) replace them. Every run
starts from the same random seed, so it can be repeated exactly.

**Who they are.** 1,000 families, each with a box. 2 to 6 people per family
(3.5 on average), each with 1.5 phones or laptops: about 5,000 devices.

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
Only daily life is sped up. The small "still here" messages, router
timeouts and reconnects run at normal speed, because those are what really
break. Uploads and
downloads keep their real sizes.

**What we measure:**
- how long until the first data arrives, and until the connection is direct
- each path's round-trip time, path switches, and how much data went direct
  or through the relay, read from iroh the same way the app does
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

## Benchmarks, on every change

Like iroh's own benchmarks ([perf.iroh.computer](https://perf.iroh.computer)),
a fixed set of tests runs on every change to `main`, and on request from a pull
request. A change that makes ZOOMRIYA Connect slower shows up before it's
released, not after. Unlike the three stages above, the benchmarks never stop:
they keep running after release.

They use the network lab's pretend routers, on one machine. Each test runs
twice more on the same network: once with plain HTTPS to the box, and once
with iroh alone, without our code. That shows what ZOOMRIYA Connect itself
adds, and what's iroh's.

**The networks:**

| Network | Conditions |
|---|---|
| Good | 20 ms delay, no loss, 1 Gbit/s |
| Far away | 200 ms delay, 1% loss |
| On a train | 30 Mbit/s, 2% loss, sudden pauses of several seconds |
| Box behind an easy router, phone on mobile data | Hole punching has to work |
| Both behind strict routers | Must stay on the relay |
| Office that blocks UDP | The relay over HTTPS, or a forwarded TCP port |

**The paths:** every test runs direct, and again forced onto the relay.

**What we measure:**

| Number | Unit | What it answers |
|---|---|---|
| Time to connect | ms | Does the app open quickly? |
| Time to direct | ms | How long does hole punching take? |
| Round-trip time, direct and on the relay | ms | How much slower is the relay? |
| Time to first byte of a page from the box | ms | How quick does the app feel? |
| Download and upload speed | Mbit/s | Are photos and videos quick? |
| Time to reconnect after a network change | ms | Does Wi-Fi to mobile data hurt? |
| Couldn't connect, and why | % of attempts | Does it get in at all? |
| Dropped, and why | per hour of use | Does it stay up? |
| Guest links that open, and which way | % of opens | Can guests see the album? |
| The relay's processor and memory, per 1,000 connections | %, MB | What will relays cost? |

Each number is the middle of 5 runs. Every run is kept, by commit, and drawn
over time on a public page like iroh's. If a number gets more than 10% worse
than on `main`, the check fails and says so on the pull request.

**After release,** small probes in several regions also measure the real
service every 5 minutes, and alert us when it gets slow:

| Probe | What it measures |
|---|---|
| Each of our relays | Round-trip time, time to connect, and failed or dropped connections, from each region |
| A test box at the team's home | Time to connect, time to direct and speed, through our real relays |
| The address server and DNS | How long an answer takes, and how many fail |
| A test guest link | Opens through the door, WebRTC and the relay, and how long the first photo takes |

The probes use test boxes and test devices only. Families' own numbers stay on
their boxes (see [Measuring connections](architecture.md#measuring-connections)).

## The goals

These are suggestions. We'll agree them before stage 2.

- **Quick to open:** 99.5% of connections open within 3 seconds, from any
  network that can reach the relay. Most see their first data in under a
  second.
- **Mostly direct:** at least 85% of family data, not counting backups, goes
  direct. The number of families stuck on the relay is within 25% of what the
  model predicted.
- **Backups:** none ever goes through our relay.
- **Stays up:** under 0.5% of attempts fail on any network that can reach
  the relay, and under one drop per 10 hours of use that the app doesn't
  recover from by itself. Every failure and drop has a reason, never
  "unknown".
- **Recovers fast:** after a home's address changes or a relay restarts,
  connections are back within 30 seconds.
- **The relay copes:** under half its processor at the busiest hour. Memory
  stays flat over two months. Its traffic and cost are within 25% of the
  model.
- **Only our servers:** recorded network traffic shows nothing going to iroh's
  public servers.
- **Strangers stay out:** the relay refuses every unpaired device, every
  expired or copied pass, and every pass from a box it doesn't know. A device
  reaches only the box that signed its pass.
- **Removed means removed:** a removed device is refused by the box at once,
  and by the relay within 5 seconds, or as soon as the box reconnects.
- **Passes renew:** no family device loses the relay because its pass ran
  out while it was in regular use. The real phones include an iPhone whose
  app is opened only once every three weeks; its pass still renews in the
  background. If it doesn't, passes get longer.
- **Certificates:** every box renews its certificate.
- **No slowdowns:** no benchmark gets more than 10% worse than on `main`
  without a reason written in the change.

## What this can't tell us

Which networks real families actually have. The lab only tests the mix we
guessed. Real numbers will come from the first homes, through connection
reports that are anonymous and only sent if the family agrees.

## Where the code goes

```
sim/
  model/   the pretend families and the traffic model (stage 1), runs in CI
  lab/     the namespaces, the pretend routers, and the code that plays out the families' days (stages 2 and 3)
  soak/    the test servers, the family machines, the dashboard and alerts (stage 3)
  bench/   the benchmarks: the networks, the comparison runs, the results page, and the probes after release
```
