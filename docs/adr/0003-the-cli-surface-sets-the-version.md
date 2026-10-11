# ADR 0003: The CLI surface sets the version

- Status: accepted
- Date: 2026-10-06

## Context

dataseek is a binary with `publish = false`; nothing depends on it through Cargo. Cargo's SemVer rules for `0.x` (minor is breaking, features bump patch) protect library dependents, and release-plz follows them by default. Under that default a new subcommand ships as `0.3.2`, indistinguishable from a bug fix.

## Decision

The version follows the command-line surface: subcommands, flags, output formats and exit codes.

| commit | before 1.0 | from 1.0 |
|---|---|---|
| `feat:` | minor | minor |
| `fix:`, `perf:`, `refactor:` | patch | patch |
| `feat!:`, `fix!:` or a `BREAKING CHANGE:` footer | minor | major |
| `docs:`, `build:`, `chore:`, `test:`, `ci:` | no release | no release |

`features_always_increment_minor = true` and `release_commits = "^(feat|fix|perf|refactor)"` in `.github/release-plz.toml` encode this. `1.0.0` is a deliberate step, taken with `release-plz set-version 1.0.0` once the surface is declared stable. Personal-project rule: stable versions only, no prerelease channels.

## Consequences

- Before 1.0 a feature and a breaking change both bump minor; the changelog's group and the `!` marker are what tell them apart, so mark breaking commits.
- Reverting the flag makes new subcommands patch releases again.
- If the crate ever publishes a library, Cargo's `0.x` rule binds and this flag violates it (release-plz warns about exactly this); revisit then.

## Evidence

- 2026-10-06, scratch clone: a `feat:` plus a `fix:` gave 0.3.1 -> 0.4.0; a lone `fix:` gave 0.3.1 -> 0.3.2.
- v0.3.1 was cut from two `ci:` merges with an empty changelog section, before `release_commits` existed.
