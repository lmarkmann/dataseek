# cli-template

<!-- repo-description -->
A starting point for a Rust CLI that already clears the cli-check quality contract: single-source version, clean pipes, honest errors, shell completion, and tests for the CLI surface itself.
<!-- /repo-description -->

Clone it, rename it, replace the `count` command with your own.

```sh
gh repo create NAME --template lmarkmann/cli-template --private --clone
cd NAME
just rename NAME
just description "WHAT THE CLI DOES"
```

See the docs in [`docs/`](docs/):

- [`setup.md`](docs/setup.md): clone, rename, and install dev tools.
- [`contract.md`](docs/contract.md): the quality contract the template satisfies.
- [`development.md`](docs/development.md): daily commands, tests, and hooks.
- [`security.md`](docs/security.md): cargo-deny and zizmor.
- [`release.md`](docs/release.md): versioning with release-plz.
- [`rejected.md`](docs/rejected.md): what was left out, and why. Read it before proposing an addition.
- [`watchlist.md`](docs/watchlist.md): what is parked as *not yet*, and the trigger that would revisit it.

## Quick commands

```sh
just check   # fmt --check + clippy -D warnings + tests
just audit   # cargo-deny + zizmor
just run count Cargo.toml
```
