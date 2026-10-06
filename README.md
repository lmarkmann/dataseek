# dataseek

<!-- repo-description -->
Search for datasets from the terminal
<!-- /repo-description -->

Still on the scaffold from `lmarkmann/cli-template`: the `count` command stands in until the first search command replaces it.

## Quick commands

```sh
just check   # fmt --check + clippy -D warnings + tests
just ci      # everything the CI job runs: check, typos, Windows and macOS cross-check, shear, msrv, audit
just audit   # cargo-deny + zizmor
just run count Cargo.toml
```

## Docs

- [`docs/CHANGELOG.md`](docs/CHANGELOG.md): what shipped, written by release-plz.
- [`docs/adr/`](docs/adr/): why it is built this way, plus what was [rejected](docs/adr/rejected.md) and what is [parked](docs/adr/watchlist.md). Read both before proposing an addition.
- [`docs/reference/`](docs/reference/): setup, development, the CLI contract, security, and the release loop.
- [`docs/synthesis/`](docs/synthesis/): comparisons across dataset sources.
