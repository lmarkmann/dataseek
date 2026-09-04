# Setup

## Clone the template

```sh
gh repo create NAME --template lmarkmann/cli-template --private --clone
cd NAME
```

## Rename the crate

```sh
just rename NAME
just description "WHAT THE CLI DOES"
```

`just rename` asks git which files under `Cargo.toml`, `src/`, `tests/` and `docs/` contain the old name, and rewrites all of them. There is no hand-maintained list to keep in step, which is deliberate: the previous version listed six paths and had already gone stale, because `tests/*.rs` never matched `tests/cli/filters.rs`. That miss was invisible, since the test there passes either way.

Three files keep the old name on purpose, and the recipe excludes them:

- `README.md` and `docs/setup.md` name `lmarkmann/cli-template` as the template to clone from, which stays true for the clone.
- `docs/rejected.md` quotes historical error messages and decisions that named the crate at the time.

After the rewrite it re-accepts the snapshots, then greps the whole tree and **fails** if the old name survives anywhere outside those three. That check is the point: it turns "remember to add new files to the list" into something the recipe enforces.

It then deletes itself from the `justfile`, because it is single-use. It refuses to run on a dirty tree, so the rename lands as one reviewable commit and `git checkout -- .` undoes it if the verification fails.

`just description` writes the supplied summary to `Cargo.toml`, the marked paragraph in `README.md`, and the current GitHub repository. Clap reads the same Cargo field for its `about` text, so the CLI help cannot drift from the package description. The summary must fit on one line.

The remaining template scaffolding is the `count` subcommand. Replace it with the CLI's actual command before the first release.

## Install dev tools

The daily workflow assumes these are on `PATH`:

- Rust toolchain with `clippy` and `rustfmt`; the channel is pinned by `rust-toolchain.toml`
- [`just`](https://just.systems) for recipes
- [`cargo-nextest`](https://nexte.st) for tests
- [`prek`](https://prek.j178.dev) for git hooks: `prek install`
- [`cargo-shear`](https://github.com/Boshen/cargo-shear) for unused dependencies, on the commit hook
- [`cargo-insta`](https://insta.rs) for `just review` and the rename
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
cargo binstall cargo-llvm-cov cargo-mutants cargo-bloat cargo-outdated
```
