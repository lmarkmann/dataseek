# Contributing

Bug reports, fixes and new sources are welcome. For anything larger than a fix, open an issue first, so the idea is settled before the code is written. Report vulnerabilities privately, as [`docs/SECURITY.md`](docs/SECURITY.md) describes.

## Setup

[`docs/reference/setup.md`](docs/reference/setup.md) lists the toolchain and the tools the recipes call. Run `prek install` once, so formatting, clippy, `cargo-shear` and `typos` run on every commit.

## While you work

```sh
just check            # fmt --check, clippy -D warnings, tests, doctests
just test <filter>    # a subset of the tests
just review           # step through changed help and output snapshots
just ci               # everything CI gates on; run it before opening a pull request
```

A green `just ci` means a green CI run on your platform.

## Commits and pull requests

Write [conventional commits](https://www.conventionalcommits.org). The prefix decides what gets released: `feat:` cuts a minor release, `fix:`, `perf:` and `refactor:` a patch, `doc:`, `build:` and `chore:` only reach the changelog, and `test:` and `ci:` stay out of it. The full table is in [`docs/reference/release.md`](docs/reference/release.md).

Each subject becomes a line in the changelog, so write it for someone who runs `dsk`, not for a reviewer: "fix: a 403 with a spent quota reads as rate limited, not as rejected credentials" rather than "fix: map 403 to RateLimited". A pull request with several commits is merged with a merge commit, so make each commit stand on its own.

Leave the version in `Cargo.toml` and `docs/CHANGELOG.md` alone; the release tooling writes both.

## Tests

No test touches the network. Each adapter splits its request from a pure `parse`, and each `parse` is tested against a real response recorded in `tests/fixtures/sources/`, with the expected record read off the fixture by hand, never pasted from the code's own output.

When you record or re-record a fixture, never send a key, trim the arrays to a few rows, and replace any person's e-mail address with `contact@example.org`. The exact steps, and where each kind of test belongs, are in [`docs/reference/development.md`](docs/reference/development.md). Commit any new file proptest writes to `proptest-regressions/`.

## Code

The lints in `Cargo.toml` deny `unwrap`, `expect`, `panic!`, indexing, unchecked arithmetic and direct printing outside tests. Write code that satisfies them rather than adding an `allow`. Results go to stdout only through `src/output.rs`, and everything else to stderr through `src/ui.rs`. [`docs/reference/contract.md`](docs/reference/contract.md) defines the flags, streams, errors and exit codes, and a change to any of them is a change to that document.

## Adding a source

A source is one row in `SOURCES` in `src/sources.rs` and at most one module under `src/sources/`. Before writing a module, check whether a protocol adapter (CKAN, Dataverse, NADA, Socrata, STAC, SDMX, EBI) already covers it; then the source is only a row ([ADR 0005](docs/adr/0005-one-adapter-per-shared-protocol.md)).

A source is complete with:

- a recorded fixture and a `parse` test against it
- its row in the source table of [ADR 0004](docs/adr/0004-every-source-is-searched.md)
- its row in [`docs/synthesis/terms.md`](docs/synthesis/terms.md), quoting the provider's terms that allow a public tool to query it this way

## Proposing something bigger

Read [`docs/adr/`](docs/adr/) first: it records why dataseek is built the way it is. [`docs/adr/watchlist.md`](docs/adr/watchlist.md) lists what is parked and the trigger that would bring each item back; if your idea is there, say what has changed.
