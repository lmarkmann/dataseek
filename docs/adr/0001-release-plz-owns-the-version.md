# ADR 0001: release-plz owns the version, the tag and the changelog

- Status: accepted
- Date: 2026-09-25, config and changelog moved 2026-10-06

## Context

The version in `Cargo.toml`, the `vX.Y.Z` tag, the changelog section and the GitHub release have to move together. Done by hand they drift, which is why there is no hand-written `just release` recipe.

## Decision

release-plz runs on every push to `main` from `.github/workflows/release-plz.yml`, in two jobs:

- `release` tags and cuts the GitHub release. It has no concurrency group, because GitHub discards a *pending* run in a group whenever a newer one queues, whatever `cancel-in-progress` says. With `release_always = false` only the release PR's merge commit tags, so a discarded run would lose the tag with no failure anywhere.
- `release-pr` opens or updates the release PR and queues it for auto-merge. It is serialized, because two runs writing the release branch race.

Both jobs mint a one-hour token from the `lmarkmann-release` GitHub App and pass it as `GITHUB_TOKEN`. Pushes and PRs made with the workflow's own `GITHUB_TOKEN` trigger no workflow, so CI would never run on the release PR and its merge would never start the release run.

The config lives at `.github/release-plz.toml`, beside the workflow that reads it, and is passed explicitly through the action's `config` input and `--config` locally. The changelog lives at `docs/CHANGELOG.md` through `changelog_path`.

## Consequences

- A bare `release-plz update` at the repo root finds no config, runs on defaults and writes a root `CHANGELOG.md`. `just release-preview` carries the flag; use it rather than the raw command.
- Merging both jobs into one under a concurrency group reintroduces the lost-tag failure.
- Swapping the App token for `GITHUB_TOKEN` stalls the loop silently: the release PR opens, CI never runs, auto-merge never fires.
- The action's `token` input is a cargo registry token. A GitHub token put there fails with `environment variable GITHUB_TOKEN is undefined`.

## Evidence

- v0.3.1 was cut end to end by this loop on 2026-09-25, after the first run failed on the `token` input mistake.
- 2026-10-06, scratch clone with a `feat:` and a `fix:` commit: `release-plz update --config .github/release-plz.toml` moved 0.3.1 to 0.4.0, wrote the section at the top of `docs/CHANGELOG.md`, and created no root `CHANGELOG.md`.
