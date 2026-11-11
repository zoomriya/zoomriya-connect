# Running the service

Part of [ZOOMRIYA Connect](../README.md).

This page is for the people who run ZOOMRIYA's servers. Families who
[run their own server](sharing-and-names.md#your-own-server) can follow it
too, on a smaller scale.

The numbers on this page (how often, how fast) are suggestions, like the
[goals in testing](testing.md#the-goals). We'll agree them before release.

## What we run

| Server | Where | What it holds | If it's lost |
|---|---|---|---|
| **Control plane** | One server: `ns1.zoomriya.net`, the address server, registrations, the outside check, guests' setup messages | Registered names and their keys, the DNS zone, address records | Restore from backup (below). Families keep connecting meanwhile: see [When things go wrong](architecture.md#when-things-go-wrong) |
| **Second DNS server** | `ns2.zoomriya.net`, at another hosting company | A copy of the zone | Set up again. It copies the zone from the first |
| **Relays** | At least two regions | Nothing that needs keeping | Boxes move to another relay. Set up again |
| **Guests' relay** (TURN) | Beside the relays | Nothing that needs keeping | Guests use the door or WebRTC meanwhile. Set up again |
| **Guest page** | `share.zoomriya.net`, static | Nothing: it's built from the repository | Publish it again |
| **Probes** | Small servers in several regions | Nothing | Set up again |
| **Benchmark machine** | One machine of our own, always the same, so results compare | Results by commit | Results are also copied to the backups |

## Backups and restore

| What | How often | Kept | Restore |
|---|---|---|---|
| **Registrations:** names, box keys, the DNS update keys' public halves, factory tokens used, released names waiting their 90 days, reported and switched-off names | Every hour, encrypted, to another hosting company | 30 days | From the latest backup, at most an hour old |
| **The DNS zone** | Not backed up | | Rebuilt from the registrations. `ns2` keeps answering from its copy meanwhile |
| **Address records** | Not backed up | | Boxes publish them again by themselves within minutes. Apps keep the last one |
| **Removed-device lists on the relays** | Not backed up | | Each box sends its list again when it reconnects |
| **Our keys** (below) | When they change | Offline, held by two people | By hand |
| **Benchmark results** | Daily | For good | From the backup |

| Goal | Suggested |
|---|---|
| Most data we can lose | One hour of registrations |
| Control plane back after a total loss | Within 4 hours |
| Restore tested, on a spare server | Every month, and after any change to how backups work |
| Alert if the last backup is older than | 2 hours |

A box that registered in the hour that was lost registers again by itself,
with the same name, when it finds it's missing. Nobody else can take that name
meanwhile: after a restore, a name with a certificate in the public logs from
the last 90 days goes only to a box that proves it holds that certificate's
key.

## Watching it

Every server's numbers go to one dashboard, reachable only by the team. The
relays' numbers (port 9090) and the control plane's are never on the internet.

| What | Alert when |
|---|---|
| The [probes](testing.md#benchmarks-on-every-change): each relay, a test box, DNS, a test guest link | Any probe fails twice in a row, or gets more than 50% slower than its usual |
| Relays: processor, memory, connections, data | Over 70% processor for 10 minutes. Memory still growing after a day |
| Refused connections on the relays | Five times the usual rate: someone may be trying passes |
| New registrations | Five times the usual rate, or many from one address |
| DNS answers | Any server not answering, or answers taking over 100 ms |
| Our certificates (`relay.`, `share.`, the control plane) | 21 days before they run out |
| Backups | Older than 2 hours, or a monthly restore test failed |
| Disk space | Over 80% |
| Benchmarks on `main` | A number 10% worse than last week |

## When something breaks

| Step | What happens |
|---|---|
| 1 | An alert goes to whoever is on call: a rota of at least two people, alerted on their phones |
| 2 | They answer within 15 minutes for urgent problems, any hour |
| 3 | If families are affected, they say so on the status page (below), in plain words: what doesn't work and what still does |
| 4 | They fix it, and update the status page as it changes |
| 5 | Within a week, a short write-up: what happened, why, what changes so it doesn't happen again. Published if families were affected |

| How urgent | Examples |
|---|---|
| **At once, any hour** | DNS not answering. Every relay down. The control plane down for over 15 minutes. A security problem |
| **Within the day** | One relay down (boxes move to another). The second DNS server down. Backups failing |
| **Next working day** | Benchmarks slower. One probe region failing alone |

**The status page** is at `status.zoomriya.com`, hosted at a different company
from all our servers, so it stays up when they don't.

**Security problems** follow [SECURITY.md](../SECURITY.md). If families' data or
keys may be affected, we tell them within 72 hours, and the authorities when
the law requires it.

## Protecting our DNS server

Our DNS server answers the whole internet for `zoomriya.app`, and for
families' own domains handed to us, so it has to
cope with floods, and must never be used to flood others.

| Risk | What we do |
|---|---|
| Being flooded with questions | Answers limited per asking address (response rate limiting). The two servers are at different companies, each with its own flood protection |
| Being used to flood someone else with big answers | Answers only for our own names, never anyone else's (no recursion). Small answers. Rate limiting per address. Repeated big answers are cut short, so the asker has to come back over TCP |
| Someone changing a box's name | Each box can change only its own name, with its own DNS update key |
| Someone copying the whole zone | Zone transfers only to `ns2`, with a shared key |

The relays have their own limits: connections per address, and speed per
connection, besides the [per-box limits](architecture.md#the-relay-on-a-server).

## Changing our keys

The keys families hold are covered in
[Architecture](architecture.md#keys-and-who-trusts-what). These are ours.

| Key | Held by | Changed | If it leaks | Families notice |
|---|---|---|---|---|
| **DNS update keys** (one per box) | The box. The control plane holds only the public half | Not needed: each works for one name only | A leaked public half is harmless. If a box's key leaks, that box gets a new one at its next check-in | Nothing |
| **Zone transfer key** (`ns1` to `ns2`) | Both DNS servers | Every year | Change it on both | Nothing |
| **Our servers' certificates** (`relay.`, `share.`, the control plane) | Each server | Automatically, every 60 days | Revoke, and get new ones at once | Nothing |
| **Guests' relay secret**, which makes the one-hour credentials | The control plane and the guests' relay | Automatically, every month. Old and new both work during the change | Change it at once. Links in use reconnect | A guest may wait a few seconds |
| **Control plane to relay key**, for the list of registered boxes | The control plane and each relay | Every year | Change it at once | Nothing |
| **Factory tokens** | Made at the factory. The control plane keeps which ones were used | Never: each is used once | Void the batch's unused tokens. Those boxes still register, with normal limits, until they get new tokens in an update | Higher relay limits wait for an update |
| **Logins to our servers** | Each team member, their own | When they leave, the same day | Remove it, and check what it did | Nothing |
| **The backup key** | Offline, two people | Every year | New key, and new backups at once. Delete the old ones | Nothing |

## What our servers keep

How long each server keeps what it sees is still to be decided. It goes on
the privacy page before release, and in [Security](security.md#the-relay).

| Server | What it sees | Kept for |
|---|---|---|
| **Relays** | Which device connects to which box, from which address, when, and how much data | To be decided |
| **Control plane** | Registered names. When a box's record is looked up, and from where | To be decided |
| **DNS servers** | Who asks for which name | To be decided |
| **Guest page** | That the page was opened, and from where | To be decided |
| **Anonymous reports**, if families agree | Totals and kinds of network, never a box or device | To be decided |

## The benchmark results page

A public page under `zoomriya.com`, like iroh's, built from the results kept
by commit. It's rebuilt after every benchmark run. The benchmarks themselves
are described in [Testing](testing.md#benchmarks-on-every-change).
