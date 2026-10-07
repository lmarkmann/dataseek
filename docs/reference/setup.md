# Setup

## Keep the description in step

```sh
just description "WHAT THE CLI DOES"
```

`just description` writes the supplied summary to `Cargo.toml`, the marked paragraph in `README.md`, and the current GitHub repository. Clap reads the same Cargo field for its `about` text, so the CLI help cannot drift from the package description. The summary must fit on one line.

## Install dev tools

The daily workflow assumes these are on `PATH`:

- Rust toolchain with `clippy` and `rustfmt`; the channel is pinned by `rust-toolchain.toml`
- [`just`](https://just.systems) for recipes
- [`cargo-nextest`](https://nexte.st) for tests
- [`prek`](https://prek.j178.dev) for git hooks: `prek install`
- [`cargo-shear`](https://github.com/Boshen/cargo-shear) for unused dependencies, on the commit hook
- [`cargo-insta`](https://insta.rs) for `just review`
- [`cargo-msrv`](https://gribnau.dev/cargo-msrv/) for `just msrv`
- [`cargo-deny`](https://embarkstudios.github.io/cargo-deny/) and [`zizmor`](https://woodruffw.github.io/zizmor/) for `just audit`
- [`release-plz`](https://release-plz.dev) for `just release-preview`
- [`gh`](https://cli.github.com) for `just description` and `just actions-outdated`, both of which talk to GitHub

[`typos`](https://github.com/crate-ci/typos) also runs on every commit, but is not on this list: `prek` fetches it from a pinned tag, so it needs nothing on `PATH`.

Install the Rust tools with `cargo binstall`:

```sh
cargo binstall cargo-nextest cargo-shear cargo-insta cargo-msrv cargo-deny zizmor release-plz
```

The on-demand recipes need four more, worth installing only when you reach for them:

```sh
cargo binstall cargo-llvm-cov cargo-mutants cargo-bsize cargo-outdated
```
