# Supply chain

dataseek runs two supply-chain gates in CI and locally through `just audit`.

## cargo-deny

`cargo-deny` is configured in `deny.toml`.

- `advisories`: RUSTSEC advisories are errors by default. Add an ID with a note and a date only when there is no fix and the code path is unreachable.
- `licenses`: only permissive licenses are allowed. The allowlist is deliberately wider than the current dependency tree so a new dependency does not fail on a license that was always going to be fine.
- `bans`: duplicate versions are warned, not failed. Wildcards in dependency requirements are denied.
- `sources`: only crates.io is allowed.

Run it with `just deny`.

## zizmor

`zizmor` audits the GitHub Actions workflows themselves. There is no `.github/zizmor.yml`: every rule runs at its default.

Every `uses:` in both workflows is pinned to a full commit SHA with its tag in a trailing comment (`actions/checkout@3d3c... # v7.0.1`). The earlier split, tags in `ci.yml` and SHAs only in `release-plz.yml`, rested on hash pins rotting without a bot to refresh them; Renovate's `helpers:pinGitHubActionDigests` is now that bot, with a seven-day `minimumReleaseAge` on action updates so a pin never moves to a release younger than a week. That removes the condition the split was built on. The trailing comment is still load-bearing: `just actions-outdated` reads it, and `pinact run --verify --check` fails on a SHA whose comment does not match. Re-pin by hand with:

```sh
just repin
```

CI runs zizmor as the `workflows` step of the single `linux` job, installed at the version the `taiki-e/install-action` pin carries, with the job's read-only token so its online audits (impostor commits, known-vulnerable actions) run too. `just zizmor` runs offline, so CI audits strictly more than a laptop; a finding that appears only in CI is one of the online audits. The same step runs `actionlint` over the workflow syntax and the shell inside `run:` blocks.

There are no rule exemptions. `artipacked` was once ignored for `release-plz.yml`, on the assumption that release-plz needed its checkout credentials to push; it pushes through the GitHub API instead, so every checkout in the repo sets `persist-credentials: false`. The two cases that would need the credentials back are signed tags and a step that runs `git push` after release-plz.

Run it with `just zizmor`.

## Workflow permissions

Workflows use the smallest permissions they can:

- `ci.yml` sets `permissions: {}` at workflow level and grants the `linux` job `contents: read`.
- `release-plz.yml` sets `permissions: {}` and grants each job `contents: read` only. Writes go through the release App's token, minted per job with Contents and Pull requests write and revoked when the job ends. Each job starts with `step-security/harden-runner` in audit mode, which logs its outbound connections.

Every checkout sets `persist-credentials: false`, including the two in `release-plz.yml`.
