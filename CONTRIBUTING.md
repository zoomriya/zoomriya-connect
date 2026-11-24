# Contributing to ZOOMRIYA Connect

Thank you for helping. ZOOMRIYA Connect carries families' private
connections, so every change is reviewed with care.

## Before you start

| Step | What to do |
|---|---|
| Security problem | Do not open an issue or pull request. Report it privately, as [SECURITY.md](SECURITY.md) says |
| A bigger change | Open an issue first and describe the problem, so we can agree on the approach before you write code |
| The design | Read [Direct+](docs/direct-plus.md): its rules (§2) apply to every change. In short: no ZOOMRIYA server, no relay or VPS, and no fallback to one |

## Signing the CLA

Every contributor signs the [Contributor Licence Agreement](CLA.md) once,
before their first pull request is merged.

| You contribute | How you sign |
|---|---|
| As yourself | Add this line to your first pull request's description: `I have read the ZOOMRIYA CLA and I agree to it.` |
| For your employer | Your employer signs first: open an issue titled "Company CLA", and we send the company version |

**Why we ask:** this code is AGPL-3.0 for everyone. ZOOMRIYA Inc. also uses
it in the ZOOMRIYA phone app, whose other code is not open. The CLA lets
ZOOMRIYA Inc. do that with your contribution too. You keep the copyright in
your work, and your contribution stays available to everyone under the AGPL.

## Making a change

| Check | Command |
|---|---|
| Build | `cargo build` |
| Tests | `cargo test` |
| Lints | `cargo clippy --all-targets` |
| Dependency licences | `cargo deny check` (new dependencies must use a licence `deny.toml` allows) |

| Rule | |
|---|---|
| Tests | Add or update tests with every change; [docs/testing.md](docs/testing.md) explains which |
| Docs | Update the page in `docs/` that describes what you changed |
| Commits | Small and focused, with a message that says what changed and why |
| Words people see | Plain and short. Never name Tor or the DHT in anything a family sees (the apps, Box Desktop); technical docs may |

## Licence

By contributing, you agree that your contribution is licensed under
[AGPL-3.0-only](LICENSE-AGPL-3.0) and under the terms of the [CLA](CLA.md).
The ZOOMRIYA name and logo are covered by [TRADEMARKS.md](TRADEMARKS.md).
