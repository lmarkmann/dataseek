# Development

## Daily commands

```sh
just         # list recipes
just check   # fmt --check + clippy -D warnings + nextest
just test    # cargo nextest run
just run search sea surface temperature
just build   # release build
just ci      # everything CI gates on, before you push
```

`check` is the inner loop and runs in seconds. `ci` is `check` plus the unused-dependency, MSRV, and supply-chain gates, so a green `just ci` means a green pipeline **on your platform**. CI is one Linux machine. A `cfg` boundary can still fail to compile on a platform nobody develops on, so the `cross-check` step (and `just cross`) runs clippy for the Windows and macOS targets from that machine ([ADR 0002](../adr/0002-one-linux-ci-job.md)).

## Format and lint

`prek` runs `cargo fmt`, `cargo clippy`, `cargo shear`, and `typos` on every commit. Install it once with `prek install`. `typos` also runs as a CI step, because a commit hook is advisory: `--no-verify`, an edit made in the GitHub web UI, or a contributor who never ran `prek install` all reach `main` without it.

The clippy lints are configured in `Cargo.toml`, not in CI, so `cargo build`, the editor, `prek`, and CI all report the same thing. The few test-specific exceptions live in `clippy.toml` because an unwrap in a test is the assertion, and the same goes for an `expect`, a `panic!`, and a `v[0]`. `arithmetic_side_effects` has no such key and does not need one: a clone that does real numeric work will meet it the first time it writes `a + b`, which is the moment to decide between `checked_`, `saturating_`, and a local `#[expect(...)]` with the reason the overflow cannot happen. One more sits at the top of `tests/cli.rs`, as a file-level allow: `clippy.toml` exempts `#[test]` bodies, and the helpers in that file are not inside one.

`typos` is the only hook that reads prose rather than code, and it is there because the doc comments in `src/cli.rs` *are* the `--help` text, so a typo in one is a user-visible defect that fmt, clippy, and shear all look straight past. It earned its place on the first run by catching a hyphenated misspelling of "misfire" in a comment in `fs.rs`. (Quoting the misspelled form here would trip the hook, which is the check working.) Unlike the other three it needs nothing on `PATH`: `prek` fetches it from the tag pinned in `prek.toml`, so a clone gets the check without a seventh tool to install. When it flags a word that is genuinely correct, prefer rewording; reach for a `typos.toml` allowlist only for a term you will use repeatedly, such as a domain word or a proper noun.

## Tests

The CLI surface is tested in `tests/cli.rs` with `assert_cmd`, and help output with [insta](https://insta.rs) snapshots. Everything below the CLI is tested in `#[cfg(test)]` modules beside the code: each source adapter against a recorded response, the search loop and the catalog fallback against fake adapters and a scratch cache, the HTTP client against a local socket, and the record, DOI and merge helpers with [proptest](https://proptest-rs.github.io/proptest/) properties as well as examples.

After changing help text or any printed output, run:

```sh
just review  # step through changed snapshots
just bless   # accept them all, unread
```

CI runs the same nextest as `just test`. nextest does not run doctests, so CI and `just check` run `cargo test --doc --locked` beside it: the crate has a library target since `dsk` joined `dataseek` ([ADR 0010](../adr/0010-dsk-is-dataseek.md)).

No test touches the network. `tests/cli.rs` gives every run its own home, config and cache directories and routes HTTP through a proxy on a closed port, which is exactly how an offline machine looks to dataseek. The HTTP tests in `src/http.rs` talk to a listener on `127.0.0.1` that answers with canned responses. Live behavior is what `dataseek bench` measures.

Every adapter splits its request (`search` or `list`) from a pure `parse`, and every `parse` has one test against a real response stored in `tests/fixtures/sources/<module>.{json,html,xml,txt}`. The test compares the first record field by field, as one `Dataset` literal, with values read off the fixture by hand. Never paste the test's own output in as the expectation: an expectation the code produced cannot catch the code being wrong, and a hand-written fixture built from the adapter's own JSON pointers cannot catch a pointer the real API never fills, which is how most adapter bugs so far were found. One table in `src/sources.rs` holds every live `parse` to the two rules in that module's header: an unrelated body is `SourceError::Shape`, never an empty list, and the result stops at `limit`.

To re-record a fixture after a source changes, repeat the adapter's request with `xh` and the dataseek User-Agent (`dataseek/<version> (mailto:user@dataseek.dev)`), never with a key, trim arrays to a few rows with `jq 'walk(if type=="array" then .[:4] else . end)'`, shorten long descriptions to a real prefix, replace any individual's e-mail address with `contact@example.org`, and then re-derive the expected record by reading the new file. Sources that need a key (FRED, Roboflow) use the example response from their API docs, linked above the test.

proptest writes the inputs that once failed to `proptest-regressions/`; they are committed so every run replays them first.

Not every test can live in `tests/cli.rs`. That file runs the compiled binary, so it can assert on flags, exit codes, and stdout, but it cannot reach into the crate: the library's modules are private, so there is nothing for an integration test to import. Anything that needs a Rust value rather than a process is a `#[cfg(test)]` module beside the code, which is why `palette`, `ui`, `fs`, and `cli` each carry one.

The unit test in `src/cli.rs` is the one worth knowing about. It calls clap's `debug_assert()` on the built command, which walks the whole definition and rejects the mistakes that compile perfectly: a short flag used twice, a `default_value` that is not among the possible values, an arg that conflicts with itself. Left uncaught, those panic inside clap the first time a user reaches the broken path, which is precisely the runtime panic `panic = "deny"` cannot see, because the panic is in the dependency and not in this crate. Add a flag, and this test is what tells you the flag is well formed before a user does.

## Benchmarks

```sh
just bench --save-baseline main   # Criterion wall time on this machine, saved as "main"
just bench --baseline main        # after a change: the difference, with an interval
just bench catalog/search         # one group; the filter is a regex over names
```

`benches/search.rs` measures the CPU stages of a search at three input sizes each: SDMX and Eurostat catalog parsing, loading a cached catalog, local catalog search, merging 76 source lists, and cleaning remote text, both HTML and prose full of bare ampersands. The library is private, so the bench reaches these through `dataseek::internals`, which exists only with the `internals` feature. `just bench` turns the feature on; a bare `cargo bench` skips the target.

CI runs the same benches through CodSpeed's CPU simulation on every push to main and on pull requests that touch code ([ADR 0012](../adr/0012-benchmarks-criterion-local-codspeed-ci.md)). The simulation counts instructions instead of timing, so its numbers hold steady across runs but are not milliseconds; compare them only with other CodSpeed runs. `cargo codspeed` measures only on Linux. Local wall time is a rough guide: an A/A run on a busy Mac reported identical code up to 52% faster ([baseline](../bench/2026-10-06-baseline.md)). Measured results go in [`../bench/`](../bench/) with the machine they came from.

`merge` takes ownership of the per-source lists, so the merge bench hands it a fresh copy made outside the measurement and cannot see a copy on the way in. That one is a test instead: `ranking_moves_the_records_instead_of_copying_them` in `src/find.rs` counts the bytes allocated between the search loop and printing with `allocation-counter`, which swaps in a counting allocator for the unit-test binary only, and fails if they reach the size of the records themselves.

Startup is measured separately, on the release binary:

```sh
just bench-startup           # hyperfine over --version, --help, the overview, completion fish, sources
just bench-startup --bless   # rewrite this machine class's baseline
```

`scripts/bench_check.py` reads hyperfine's export (`docs/bench/startup.json`) and fails on any path over its budget: 10 ms for `--version`, 20 ms for the rest, doubled under `CI` because runners are slower and noisier. A path more than 25% and 2 ms slower than `docs/bench/baseline-<os>-<arch>.json` only warns. The CI job runs it after the tests and uploads the JSON, so a slow path can be traced to its commit.

## Dependencies

```sh
just shear             # declared but never used, also a commit hook
just outdated          # everything this repo pins
just crates-outdated   # only the crates
just actions-outdated  # only the action and hook tags
```

`cargo shear` runs on every commit because it takes milliseconds and catches the dependency you stopped using two commits ago.

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

`just bloat` runs [cargo-bsize](https://crates.io/crates/cargo-bsize) on the `dataseek` binary (`dsk` is the same program). It ranks crates, functions and generic instantiations by the bytes they ship, and lists panic, formatting and unwind overhead beside them. Before a change that might move the size, run `just bloat` and copy the binary it builds, `target/bsize/release/dataseek`, somewhere outside `target/`; afterwards `just bloat --baseline <that copy>` reports what grew and what shrank, by crate and by function. The baseline has to be that build: the release binary is stripped, so there is nothing left in it to attribute bytes to. `just bloat --what-if` rebuilds with `opt-level = "z"` and with `panic = "abort"` and reports what each saves; `just bloat --what-if --levers=all` adds LTO mode, codegen settings and the nightly-only levers such as `fmt-debug=none` and `build-std`, one build each, so it takes minutes. Nightly-only levers are measured for reference and cannot go into a profile while the toolchain is pinned to stable. The last measurement is in [`../bench/2026-10-07-binary-size.md`](../bench/2026-10-07-binary-size.md).

`just mutants` passes `--in-diff` so it stays in the minutes rather than mutating the whole crate. A surviving mutant means a line the tests execute but never assert on, which is the failure mode coverage percentages hide.
