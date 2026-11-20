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
| **Relays** | At least two regions, each with TURN beside it on the same server and address ([TURN beside a relay](#turn-beside-a-relay)) | Nothing that needs keeping | Boxes move to another relay. Set up again |
| **Guest page** | `share.zoomriya.net`, static | Nothing: it's built from the repository | Publish it again |
| **Probes** | Small servers in several regions | Nothing | Set up again |
| **Benchmark machine** | One machine of our own, always the same, so results compare | Results by commit | Results are also copied to the backups |

## Installing

Both servers are single programs, run by systemd on Debian 13. Example files
are in [`deploy/`](../deploy).

| Step | Control plane | Relay |
|---|---|---|
| 1. Build | `cargo build --release -p zoomriya-control-plane` | `cargo build --release -p zoomriya-relay` |
| 2. Program | `/usr/local/bin/zoomriya-control-plane` | `/usr/local/bin/zoomriya-relay` |
| 3. User | `deploy/zoomriya-connect.sysusers` to `/usr/lib/sysusers.d/zoomriya-connect.conf`, then `systemd-sysusers` | The same |
| 4. Settings | `deploy/control-plane.toml` to `/etc/zoomriya-control-plane/` | `deploy/relay.toml` to `/etc/zoomriya-relay/`. On each relay, set `name` and the certificate's domain to its own: `relay1.zoomriya.net`, `relay2.zoomriya.net` |
| 5. Keys | `relay-keys`: one control plane to relay key a line, at least 32 characters (`openssl rand -base64 48`) | `feed.key`: one of those keys |
| 6. Service | `deploy/zoomriya-control-plane.service` | `deploy/zoomriya-relay.service` |
| 7. Start | `systemctl enable --now zoomriya-control-plane` | `systemctl enable --now zoomriya-relay` |
| 8. GUI and account page | `cd admin-web && npm ci && npm run build`, then `dist/` to `/usr/share/zoomriya-connect/admin` and `dist-account/` to `/usr/share/zoomriya-connect/account` | `dist/` only |
| 9. Sign-in token | `zoomriya-control-plane admin-token --config FILE NAME` | `zoomriya-relay admin-token --config FILE NAME` |

| Other server | How |
|---|---|
| **Second DNS server** (`ns2`) | The same program: `deploy/control-plane-ns2.toml` and `deploy/zoomriya-ns2.service`. Give it one of the first server's `zone-keys` |
| **TURN** | On each relay's server: see [TURN beside a relay](#turn-beside-a-relay) |

| Port | Server | Open to |
|---|---|---|
| TCP 443 | Both | Everyone |
| UDP and TCP 53 | Control plane | Everyone |
| TCP 80 | Relay | Everyone: the captive-portal check only |
| UDP 7842 | Relay | Everyone: QUIC address discovery |
| TCP 9090 | Relay | The server itself only: numbers for the dashboard |
| TCP 8444 | Both | The server itself only: the GUI. Reach it with `ssh -L 8444:127.0.0.1:8444 server` |
| UDP and TCP 3478 | Relay (coturn) | Everyone: TURN, and STUN for boxes and laptops ([Laptops](laptops.md#connecting)). TURN over TLS comes in on the relay's TCP 443 |

Certificates come from Let's Encrypt over port 443 by themselves, or from
files, read again every hour. The services run with only the right to use
ports below 1024.

### Operator commands

They work on the control plane's database directly. A running server notices
within a second. The GUI does the same things.

| Command | What it does |
|---|---|
| `zoomriya-control-plane status --config FILE` | How many boxes: all, with a factory token, stolen, switched off |
| `zoomriya-control-plane factory-tokens --config FILE TOKENS` | Adds factory tokens, one a line, each with its box's ID. Only the tokens' hashes are kept |
| `zoomriya-control-plane switch-off --config FILE NAME` | The name stops answering, after a report. `switch-on` undoes it |
| `zoomriya-control-plane mark-stolen --config FILE NAME` | Relays refuse the box and its devices within seconds |
| `zoomriya-control-plane reports --config FILE` | Reports not yet dealt with. `report-done ID` closes one |
| `zoomriya-control-plane accounts --config FILE` | Every ZOOMRIYA Connect account, with its boxes |
| `zoomriya-control-plane account-new --config FILE USERNAME` | A family's account, for up to 3 boxes. Shows its password once |
| `zoomriya-control-plane account-reset --config FILE USERNAME` | A new password, shown once. The old one and every sign-in stop working |
| `zoomriya-control-plane account-block --config FILE USERNAME` | Its boxes stop working away from home within seconds; it can't sign in. `account-unblock` undoes it |
| `zoomriya-relay enrol-code --config FILE` | A family's own relay only: a one-time code for Box Desktop |
| `admin-token --config FILE NAME` (either server) | A sign-in token for the GUI, shown once. Remove its line from the tokens file to take it back |

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

### TURN beside a relay

Each relay's server also runs TURN (Debian 13's `coturn`), for laptops whose
WebRTC can't connect directly, and for guests. **One server, one address:**
the relay's front door shares port 443 by the name each connection asks for.

| Connection on TCP 443 asks for | Goes to |
|---|---|
| `relay1.zoomriya.net` (or anything else) | The relay, as always |
| `turn.zoomriya.net` | The relay opens the TLS with its own certificate, which covers both names, and passes the TURN stream to coturn on `127.0.0.1:3478`. WebRTC's own encryption between the laptop and the box runs inside it |
| A `[[pass_through]]` name, such as a family's `connect.smith.ca` | Passed on unopened to the program that answers it on the same server, which keeps its own certificate |

| Step | On the relay's server |
|---|---|
| 1 | `apt install coturn`, then `deploy/turnserver.conf` to `/etc/turnserver.conf`. Add our own servers' addresses as `denied-peer-ip` |
| 2 | One secret, at least 32 characters (`openssl rand -base64 48`), in three places: coturn's `static-auth-secret`, `/etc/zoomriya-relay/turn-secret`, and the first line of the control plane's `turn-secrets` (guests' credentials) |
| 3 | Let the relay read coturn's log: `mkdir -p /var/log/turnserver && chown turnserver:turnserver /var/log/turnserver && chmod 750 /var/log/turnserver`, and `systemctl edit zoomriya-relay` with `[Service]` `SupplementaryGroups=turnserver`. Rotate the log with `copytruncate` (logrotate) |
| 4 | The `[turn]` section in `relay.toml`, and `turn.zoomriya.net` in the certificate's domains |
| 5 | DNS: `turn.zoomriya.net` at the relay's address. Firewall: UDP and TCP 3478 open |
| 6 | `systemctl restart coturn zoomriya-relay`. The Servers page shows the TURN server answering |

| What | How |
|---|---|
| Credentials | Boxes ask their relay (`POST /zoomriya/v1/turn`, signed with the box key). They last a day, and laptops get new ones each time they connect. The username is the box key, so coturn's records name the box |
| Counting | coturn writes each session's bytes, by username, to its log (`verbose`). The relay reads the log and reports each box's TURN bytes with its relay bytes. TURN counts towards the box's monthly allowance; it can't be slowed, only the relay can |
| Checking | The relay sends coturn a STUN request every 30 seconds; the Servers page shows the answer |

**Moving TURN to its own server later.** Boxes and laptops only know the name
`turn.zoomriya.net`, so nothing changes for them.

| Change | Where |
|---|---|
| Point `turn.zoomriya.net` at the new server | DNS |
| coturn answers port 443 itself: `tls-listening-port=443`, with its own certificate | The new server's `turnserver.conf` |
| Drop `turn.zoomriya.net` from the relay's certificate, and point `[turn] local` at the new server, for the check | The relay's `relay.toml` |
| Get coturn's log to the relay, for counting: forward it with rsyslog, or we add a TURN-only mode to `zoomriya-relay` | The new server |

## Watching it

**The GUI's Servers page** shows every server we run, each reporting every
half minute (relays with every poll):

| Server | Reports |
|---|---|
| Control plane | Itself |
| Each relay, with its TURN server | With its usual report (`/v1/relay/report`) |
| Second DNS server | `POST /v1/server/report`, with its zone-copy key |

| Shown | From |
|---|---|
| Online, or not reporting (nothing for two minutes) | When it last reported |
| Uptime, version, load, memory, disk | The machine (`/proc`, the data directory's disk) |
| Received and sent this month | The machine's network counters, added up per month; a restart starts them again without losing the month |
| Through the relay, and through TURN, this month | The boxes' counts, as for the allowance |
| The TURN server answering | The relay's check |
| The relay seen from outside | The control plane's own check of each relay |

A server shut down for good can be removed from the list; it comes back if it
reports again. The Overview shows how many are online, and the Boxes page
each box's relay and TURN use. A family's own relay shows its own server on
its GUI's front page.

Every server's numbers go to one dashboard, reachable only by the team. The
relays' numbers (port 9090) and the control plane's are never on the internet.

| What | Alert when |
|---|---|
| The [probes](testing.md#benchmarks-on-every-change): each relay, a test box, DNS, a test guest link | Any probe fails twice in a row, or gets more than 50% slower than its usual |
| Relays: processor, memory, connections, data | Over 70% processor for 10 minutes. Memory still growing after a day |
| A server not reporting (the Servers page) | Two minutes without a report |
| A TURN server not answering | Two checks in a row |
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
| Someone taking a family's own domain first | We answer for an own domain only once its registrar's NS record names that box's own name server (`<box id>.ns.zoomriya.net`) |
| Someone copying the whole zone | Only `ns2` can copy it, over HTTPS, with its own key |

The relays have their own limits: connections per address, and speed per
connection, besides the [monthly allowance](architecture.md#the-relay-on-a-server).

| Allowance setting, in the GUI | Starts at |
|---|---|
| Self-installed boxes, GB a month | 0: no limit |
| Ready-made boxes (factory token), GB a month | 0: no limit |
| Speed once past it | 1 Mbit/s |
| One box's own allowance | The usual one |

Set ours from the [soak test](testing.md) before launch. A family's own server
keeps no limit unless they set one.

## Changing our keys

The keys families hold are covered in
[Architecture](architecture.md#keys-and-who-trusts-what). These are ours.

| Key | Held by | Changed | If it leaks | Families notice |
|---|---|---|---|---|
| **DNS update keys** (one per box) | The box. The control plane holds only the public half | Not needed: each works for one name only | A leaked public half is harmless. If a box's key leaks, that box gets a new one at its next check-in | Nothing |
| **Zone-copy key** (`ns1` to `ns2`) | Both DNS servers | Every year. Old and new both work while `zone-keys` lists both | Change it on both | Nothing |
| **Our servers' certificates** (`relay.`, `share.`, the control plane) | Each server | Automatically, every 60 days | Revoke, and get new ones at once | Nothing |
| **TURN secret**, which makes TURN credentials | The control plane (`turn-secrets`), each relay (`turn-secret`) and each coturn (`static-auth-secret`) | Every month: change all three together. Guests' credentials last an hour, laptops' a day: those made with the old secret stop working | Change it at once. Links and laptops in use reconnect, and laptops get new credentials as they do | A guest or laptop may wait a few seconds |
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
