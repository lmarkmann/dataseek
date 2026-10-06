# Development

## Daily commands

```sh
just         # list recipes
just check   # fmt --check + clippy -D warnings + nextest
just test    # cargo nextest run
just run count Cargo.toml
just build   # release build
just ci      # everything CI gates on, before you push
```

`check` is the inner loop and runs in seconds. `ci` is `check` plus the unused-dependency, MSRV, and supply-chain gates, so a green `just ci` means a green pipeline **on your platform**. CI is one Linux machine. A `cfg` boundary can still fail to compile on a platform nobody develops on, so the `cross-check` step (and `just cross`) runs clippy for the Windows and macOS targets from that machine ([ADR 0002](../adr/0002-one-linux-ci-job.md)).

## Format and lint

`prek` runs `cargo fmt`, `cargo clippy`, `cargo shear`, and `typos` on every commit. Install it once with `prek install`. `typos` also runs as a CI step, because a commit hook is advisory: `--no-verify`, an edit made in the GitHub web UI, or a contributor who never ran `prek install` all reach `main` without it.

The clippy lints are configured in `Cargo.toml`, not in CI, so `cargo build`, the editor, `prek`, and CI all report the same thing. The few test-specific exceptions live in `clippy.toml` because an unwrap in a test is the assertion, and the same goes for an `expect`, a `panic!`, and a `v[0]`. `arithmetic_side_effects` has no such key and does not need one: a clone that does real numeric work will meet it the first time it writes `a + b`, which is the moment to decide between `checked_`, `saturating_`, and a local `#[expect(...)]` with the reason the overflow cannot happen. One more sits at the top of `tests/cli.rs`, as a file-level allow: `clippy.toml` exempts `#[test]` bodies, and the helpers in that file are not inside one.

`typos` is the only hook that reads prose rather than code, and it is there because the doc comments in `src/cli.rs` *are* the `--help` text, so a typo in one is a user-visible defect that fmt, clippy, and shear all look straight past. It earned its place on the first run by catching a hyphenated misspelling of "misfire" in a comment in `fs.rs`. (Quoting the misspelled form here would trip the hook, which is the check working.) Unlike the other three it needs nothing on `PATH`: `prek` fetches it from the tag pinned in `prek.toml`, so a clone gets the check without a seventh tool to install. When it flags a word that is genuinely correct, prefer rewording; reach for a `typos.toml` allowlist only for a term you will use repeatedly, such as a domain word or a proper noun.

## Tests

Tests live in `tests/cli.rs` and use `assert_cmd` for the CLI surface. Output assertions use [insta](https://insta.rs) snapshots.

After changing help text or any printed output, run:

```sh
just review  # step through changed snapshots
just bless   # accept them all, unread
```

CI runs the same nextest as `just test`. nextest does not run doctests, which costs nothing here because a binary crate has none; a clone that grows a `src/lib.rs` should add `cargo test --doc --locked` to the workflow.

Not every test can live in `tests/cli.rs`. That file runs the compiled binary, so it can assert on flags, exit codes, and stdout, but it cannot reach into the crate: a binary has no library target for an integration test to import. Anything that needs a Rust value rather than a process is a `#[cfg(test)]` module beside the code, which is why `palette`, `ui`, `fs`, and `cli` each carry one.

The unit test in `src/cli.rs` is the one worth knowing about. It calls clap's `debug_assert()` on the built command, which walks the whole definition and rejects the mistakes that compile perfectly: a short flag used twice, a `default_value` that is not among the possible values, an arg that conflicts with itself. Left uncaught, those panic inside clap the first time a user reaches the broken path, which is precisely the runtime panic `panic = "deny"` cannot see, because the panic is in the dependency and not in this crate. Add a flag, and this test is what tells you the flag is well formed before a user does.

## Dependencies

```sh
just shear             # declared but never used, also a commit hook
just outdated          # everything this repo pins
just crates-outdated   # only the crates
just actions-outdated  # only the action and hook tags
```

`cargo shear` runs on every commit because it takes milliseconds and catches the dependency you stopped using two commits ago. The template ships a few deps speculatively (`tempfile` for `fs.rs`, `indicatif` for `ui.rs`), so a clone that deletes one of those modules gets told about the leftover.

`just outdated` covers two kinds of pin, because they rot differently. `cargo outdated` reads the manifest, where requirements are major-only and the lockfile decides the build, so a stale crate is still visible to `cargo update` and a vulnerable one to `cargo-deny`. The tags in `.github/workflows/` and `prek.toml` have neither backstop: nothing in the repo resolves them and nothing says when they fall behind, which is how the `crate-ci/typos` pin came to sit a release behind in both files at once. `just actions-outdated` asks GitHub for each latest tag and compares only the components the pin declares, so `@v2` stays quiet until a v3 exists while `@v1.48.0` reports the next patch. It needs `gh` authenticated, and it skips branch pins like `@stable`, which move on their own. A hash pin is compared through the `# v7.0.1` comment `pinact` writes beside it, since a 40-character SHA carries no version to compare; a SHA with no such comment is reported as an error rather than skipped, because silently dropping an action from the freshness report is the one way pinning could leave the repo less current than tags did.

## MSRV

```sh
just msrv       # does the crate still compile on the declared floor?
just msrv-find  # what the floor actually is, when you want to move it
```

`rust-version` in `Cargo.toml` is the single source of truth, and the CI job reads the number out of the manifest rather than repeating it. This matters more than it looks: `rust-toolchain.toml` pins everyone to `stable`, so without this gate the first use of a newer-than-floor API would break the claim silently.

`just msrv-find` measures the floor; `cargo msrv find` on its own does not, which is why the recipe exists. The search works by shelling out to `cargo check`, and cargo refuses the build on the `rust-version` field before compiling a line, so a bare run never looks below the number already declared and reports it back as though it had tested it. The recipe passes `-- cargo check --ignore-rust-version` to replace the inner command; there is no flag on the subcommand itself. This is the same failure as the paragraph above, arriving from the other direction: there the gate passes for the wrong reason, here the measurement confirms itself.

The floor is a claim you choose, not the compiler anyone builds with. Everything local, every commit hook, and the main CI job run current stable no matter what the number says, and the floor only decides who is refused and which dependency versions the resolver considers. Two consequences. Setting it to whatever stable happens to be today makes the MSRV job a copy of the stable job until the next release, which on Rust's six-week train is at most six weeks of a gate that tests nothing; check [releases.rs](https://releases.rs) for where the train currently is rather than assuming. And raising it relaxes the resolver, so a later `cargo update` can pull dependency versions that were previously held back; compare `cargo update --dry-run` against `cargo update --dry-run --ignore-rust-version` to see what a bump would let in. Move the number when a language feature earns it, and if you are publishing, run `just msrv-find` and declare what the code actually needs.

If `just msrv` fails locally with *"some components are unavailable for download for channel '1.xx': 'miri' ... 'rustc-codegen-cranelift'"*, the problem is your machine, not the crate. `cargo msrv` installs the floor toolchain through rustup, and a fresh install uses whatever `profile` is set in `~/.rustup/settings.toml`. If that is `complete`, rustup asks for every component it knows about, including `miri` and `rustc-codegen-cranelift`, which are nightly-only artifacts and are never published for a stable release, so the install cannot succeed. Fix it once with `rustup set profile default`; it changes future installs only and leaves existing toolchains alone. Note that `rust-toolchain.toml` saying `profile = "minimal"` does not help here, because that line only applies when rustup installs *from that file*, and `cargo msrv` names the toolchain itself. CI never sees this: `rustup toolchain install` there reads the profile from `rust-toolchain.toml`.

That same pin is why the CI job sets `RUSTUP_TOOLCHAIN` on the step instead of trusting the toolchain it just installed. `rust-toolchain.toml` says `stable`, and rustup honors that file over any toolchain installed beside it, so installing the floor and calling `cargo check` would have quietly checked against stable and passed for the wrong reason. rustup's precedence runs `+toolchain` first, then `RUSTUP_TOOLCHAIN`, then the file, so the env var is what makes the job build against the floor. A gate that passes for the wrong reason is worse than no gate, because it also stops you looking.

## Audit commands

```sh
just deny    # cargo-deny: advisories, licenses, bans, sources
just zizmor  # zizmor: static analysis of the workflows
just audit   # both, the pair CI runs
```

`cargo-audit` is deliberately absent: `cargo deny check advisories` reads the same RustSec database, and running both means two database clones for one signal.

## On demand

Not gates, and not run by `just ci`. Reach for them when the question comes up.

```sh
just cov       # line coverage, browsable under target/llvm-cov/html
just mutants   # mutate what the branch changed against main
just bloat     # where the release binary's size goes
```

`just mutants` passes `--in-diff` so it stays in the minutes rather than mutating the whole crate. A surviving mutant means a line the tests execute but never assert on, which is the failure mode coverage percentages hide.
