# Rejected

Things that were considered and deliberately left out, with the reason and the date of the decision. The point is that a rejection stays rejected: without this file the same plausible-sounding addition gets re-proposed every few months and re-litigated from scratch.

A rejection is not permanent. If the reason stops being true (an upstream interface stabilizes, a dependency changes behavior), that is a new entry, not a silent reversal. Ideas that were never rejected, only parked as *not yet*, live in [`watchlist.md`](watchlist.md) with the trigger that would revisit them.

The template rule most of these fall out of: **anything a clone has to delete is worse than nothing**. A starter is judged by what it saves you, and a feature you did not ask for costs you a deletion plus the confidence that deleting it broke nothing.

## 2026-07-27: Dynamic completions (`clap_complete`'s `CompleteEnv`)

Needs the `unstable-dynamic` feature. `clap_complete-4.6.7/src/env/mod.rs` warns that the completions work by generating shell code that calls back into the binary, that this interface is unstable, and that a mismatch between the shell code and the binary yields either wrong completions or none. Upstream's own mitigation is to re-source the completions on every shell start rather than write them to a file.

That is the wrong trade for a template. Clones get created and then not revisited; a completion mechanism that silently degrades on upgrade fails in exactly the situation this template is built for. The static `completion <shell>` subcommand writes a file once and keeps working.

## 2026-07-27: `human-panic`, `miette`, `bugreport`, and `vergen` commit-hash versions

Each is a real improvement to *some* CLI, and each is a feature the clone has to delete when it is not that CLI. `human-panic` reframes a panic the lint config now forbids anyway. `miette` is a second error-rendering system next to the `Error:` / `Cause:` / `Try:` printer that already satisfies the contract. `bugreport` presumes an issue tracker and a user base. `vergen` puts a build-time git hash into `--version`, which breaks the single-source-version rule and makes the build depend on git metadata.

## 2026-07-27: `trycmd` / `snapbox`

Would replace a working `assert_cmd` + `insta` setup with a second snapshot system, so the template would ship two ways to assert on CLI output and no guidance on which to use. No capability gained.

## 2026-07-27: A Ctrl-C cursor-restore handler

Rejected because the bug it would fix does not exist here. The concern was a progress bar hiding the terminal cursor and a Ctrl-C leaving it hidden. Checked against the vendored source: `indicatif-0.18.6/src/` contains no `hide_cursor`, no `show_cursor`, and no `?25` escape anywhere, so it never touches the cursor and there is nothing to restore.

Worth recording precisely because the reasoning was sound and the premise was still wrong. If a future dependency does start hiding the cursor, this becomes a real requirement.

## 2026-07-27: Publishing to crates.io, binary release matrix, and Homebrew tap bump

Inherited from the `celsius` release workflow the release automation was adapted from. Deleted: the crate is `publish = false`, so there is no registry release; a five-target binary matrix and a tap bump publish a template nobody installs. A clone that does want to ship re-adds these deliberately, which is one paragraph in `release.md`, rather than inheriting a workflow that fails or publishes something unintended on its first tag.

## 2026-07-27: CodSpeed benchmark workflow

Also inherited from `celsius`. It ran `cargo codspeed build --bench render` against a `benches/` directory this repo does not have, so it could only ever fail. A benchmark harness with nothing to benchmark is not a starting point.

## 2026-07-27: GitHub releases and `cargo-semver-checks` in release-plz

`git_release_enable = false`: with no binaries to attach, a GitHub release only duplicates the tag it was cut from.

**Overturned 2026-09-25 for GitHub releases; `semver_check = false` stands.** The standing rule across the user's repositories is a tag and a GitHub release per version, the release body being that version's changelog section, so a reader of the Releases page gets the notes without opening the file. `git_release_enable = true` and `git_release_type = "auto"` now.

`semver_check = false`: cargo-semver-checks compares the public API of a library. This crate is a binary and has no public API, so the check has nothing to compare. Turning it off also removed the 15-line custom `pr_body` template, which existed solely to surface the semver marker.

## 2026-07-27: `release_always = true` (release-plz default)

The default releases on every commit to `main`. Set to `false` so a tag is cut only when the release PR merges, which is the behavior actually wanted: the version bump is reviewable as a diff before it becomes a tag.

## 2026-07-27: Hash-pinning GitHub Actions

zizmor's `unpinned-uses` audit wants full commit SHAs; `.github/zizmor.yml` relaxes this to `ref-pin`. Tags are mutable, so this is a trust decision about the publishers (`actions/`, `dtolnay/`, `Swatinem/`, `EmbarkStudios/`, `release-plz/`, `zizmorcore/`) rather than a guarantee.

Rejected because hash pins rot: they need Dependabot to refresh the SHAs, and a clone that never enables Dependabot ends up pinned to increasingly stale actions, which is worse than tag-pinning. This is machinery a template should not impose on every clone.

**Re-checked 2026-09-04, and the entry holds.** The question came back, so it was settled with a count instead of an argument: every `uses:` line in ripgrep, fd, bat, delta, hyperfine, uv, jj, watchexec, zoxide, eza, starship, difftastic, dust and tokei. Four of the fourteen SHA-pin (starship 56/56, jj 54/55, uv 484/517, difftastic 23/30); the other ten tag-pin, ripgrep and bat with a single SHA each. Three of the four SHA-pinners run a bot to keep the hashes alive, which is the dependency this entry named: uv and starship through `.github/renovate.json5`, jj through Dependabot.

The fourth is the control group. difftastic SHA-pins with no bot config and no version comments, so its workflows are twenty-three opaque hashes that cannot be read or dated. That is the predicted failure mode, observed.

Worth recording for a clone that does turn Renovate on, because it is not what jj does: uv and starship write the pin as `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1`, with the trailing tag comment maintained by Renovate's `helpers:pinGitHubActionDigests` preset. SHA-pinning without that comment is the version nobody can review.

**The count is not the whole case, and the entry should not pretend otherwise.** What ten of fourteen repos do is evidence about practice, not about correctness, and the guidance runs the other way: GitHub's own security-hardening page recommends pinning third-party actions to a full-length commit SHA, OpenSSF Scorecard's `Pinned-Dependencies` check scores tag pins down, and the 2025 `tj-actions/changed-files` compromise is the demonstration, with tags rewritten under thousands of consumers. ripgrep and fd almost certainly tag-pin because they are old repos with read-only CI, not because they weighed this and declined.

What keeps the decision where it is, for `ci.yml` specifically, is blast radius rather than a head count. That workflow runs with `contents: read`, holds no secrets, and produces nothing that ships: `publish = false`, no binary release, no tap. A compromised action there can poison a build nobody consumes. Set against that, SHA pins in a repo with no bot freeze the actions permanently, and a frozen action eventually fails outright when the runner drops its Node version.

**Partially overturned 2026-09-04, the same day.** `release-plz.yml` is now hash-pinned, and the watch-list trigger it was parked behind turned out to be the wrong test. That entry said to revisit once the repo held a second secret; the exposure did not need a second secret to arrive, because one token with `contents: write` is already enough to push to `main` and cut releases. Waiting for a second would have been waiting for a number to change rather than for the risk to.

Pinned with [`pinact`](https://github.com/suzuki-shunsuke/pinact), which writes the trailing `# v7.0.1` comment alongside each SHA. That comment is what keeps the file reviewable and what `just actions-outdated` compares against, and the recipe was taught to read it in the same commit; without that, pinning would have silently removed two actions from the freshness report. `dtolnay/rust-toolchain@stable` stays a branch ref: it names a toolchain rather than a version, and pinact refuses it for that reason.

The rest of this entry stands, and `ci.yml` stays on tags. It runs read-only, holds no secret and ships nothing, so it buys nothing from pins that a clone has no bot to refresh.

**Fully overturned 2026-09-25.** The entry's own condition was a bot to keep the hashes alive, and Renovate is that bot: `.github/renovate.json` now extends `helpers:pinGitHubActionDigests`, which writes the SHA with the `# v7.0.1` comment this entry named as the reviewable form, and a seven-day `minimumReleaseAge` holds action updates back for a week. With the bot in place every `uses:` in both workflows is hash-pinned and `.github/zizmor.yml` is gone.

## 2026-07-27: `persist-credentials: false` on the release-plz checkout

Applied to every other checkout to clear zizmor's `artipacked` finding. Not applied here: those credentials are what release-plz pushes the release branch and the tag with. `artipacked` is ignored for that one workflow in `.github/zizmor.yml`.

**Overturned the same day.** The premise was wrong, and checking it took one page of upstream documentation that was never read. `website/docs/github/persist-credentials.md` says release-plz pushes commits, branches and tags **through the GitHub API**, so the checkout credentials are not what does the pushing. It names the only two cases that need them: signed tags, which release-plz detects via `git config --get tag.gpgSign` and which this repo does not configure, and a later step that runs `git push` itself, which this workflow does not have. Upstream's own quickstart sets `persist-credentials: false` on both jobs.

So the workflow holding the strongest permissions in the repo was persisting a usable token in `.git/config` for no reason, and the `artipacked` ignore was suppressing a finding that was right. Both are gone. The lesson worth keeping is not about credentials: an audit exemption is a claim about behavior, and this one was written from an assumption about how a tool works rather than from its documentation.

## 2026-07-27: Uploading zizmor findings to the Security tab

The action's default, and the mode the CI job originally used. It uploads SARIF through `github/codeql-action/upload-sarif`, giving stateful tracking and incremental triage across runs, which is genuinely better than a console dump.

Rejected as a template default on two counts. It is not portable: the upload needs the repository to be public or to pay for Advanced Security, so a private clone gets a red X on a job that found nothing, which is what happened here on the first push. And in that mode the action deliberately does not fail the job on findings, because GitHub expects a merge-protection ruleset to do the blocking; the job would report green with findings outstanding. A gate that cannot fail is worse than no gate. `advanced-security: false` behaves the same way everywhere and matches what `just zizmor` does locally. A public clone that wants the Security tab can turn it back on.

## 2026-07-27: `unreachable_pub` and `missing_debug_implementations` lints

Both are library lints. In a binary, `pub` on an item in a private module is the normal way to expose it across modules, and the crate has no external consumers for whom `pub` means anything. `unreachable_pub` produced 29 warnings on correct code. A lint that fires on the idiomatic form trains people to ignore lint output.

## 2026-07-27: Dropping `-D warnings` from CI now that lints live in `Cargo.toml`

The manifest carries a curated list, so it cannot deny a lint nobody listed. `-D warnings` stays in `just check` and CI as the catch-all for everything not named explicitly. The manifest gives editor and `cargo build` parity; the flag is the backstop. They are not redundant.

**Holds, respelled in CI on 2026-09-25.** `ci.yml` sets `CARGO_BUILD_WARNINGS=deny` (Cargo's `build.warnings`, stable since Rust 1.97) instead of passing `-D warnings`: the same catch-all over local lints, measured failing build, clippy and rustdoc, without invalidating the build cache the way `RUSTFLAGS` does. `just check` keeps the flag.

## 2026-07-27: Removing `components:` from `ci.yml` to single-source the toolchain

`rust-toolchain.toml` names the channel and the components, so repeating `components: clippy, rustfmt` in CI looks like duplication worth deleting. It is not: `dtolnay/rust-toolchain` installs the toolchain itself, and rustup only honors a components list when it is the one performing the install. Deleting the CI line risks a runner with a toolchain but no clippy.

Kept the duplication with a comment at the site saying the two must agree. The alternative that would genuinely single-source it is dropping the toolchain step entirely and letting rustup auto-install from the file on first `cargo` invocation.

**Resolved 2026-09-25 the way this entry named.** CI no longer uses `dtolnay/rust-toolchain`: `rustup toolchain install` with no argument installs the toolchain and components `rust-toolchain.toml` names, so the file is the only list.

## 2026-07-27: Freezing a specific rustc version in `rust-toolchain.toml`

`channel = "stable"` tracks the newest stable instead of pinning a number. A frozen version in a template ages into the default for every clone, and clones are not revisited. The MSRV that actually matters is `rust-version` in `Cargo.toml`, which is a declared floor rather than the compiler everyone is forced onto.

## 2026-07-27: Tightening dependency requirements past the major version

Requirements stay major-only (`"1"`, `"4"`). The lockfile pins the actual build, so a narrower requirement in the manifest buys nothing and bakes a stale minimum into every clone.

## 2026-07-27: Backticking `NO_COLOR` and friends to satisfy `doc_markdown`

`clippy::doc_markdown` is `allow` in the manifest. The doc comments in `src/cli.rs` are the `--help` text: backticks added to satisfy rustdoc would be printed literally to the terminal. This is the one lint where the correct-looking fix makes user-facing output worse.

## 2026-07-27: Suppressing `expect_used` in `ui.rs` with `#[expect(...)]`

Enabling `clippy::expect_used` flagged two `.expect()` calls on progress-bar templates. The cheap fix was a local allow with a reason.

Rejected in favor of the real fix: the templates now fall back to indicatif's default style, so a malformed template degrades instead of killing the program mid-render, and the existing unit test still catches a typo at test time. The lint was pointing at a genuine design question ("what should happen if this fails?"), and the honest answer was not "panic".

## 2026-07-27: A hand-written `just release` recipe and a hand-edited `CHANGELOG.md`

Replaced by release-plz. The old recipe ran `cargo set-version`, committed, tagged, pushed, and published, with a comment telling you to update the changelog first, which is exactly the step a human forgets. Version bumps and changelog entries are now generated from conventional commits and land as a reviewable PR.

## 2026-07-27: Having `just rename` rewrite `README.md`

The recipe rewrites the manifest, `src/`, and `tests/`. It leaves the README name alone: most occurrences describe the template itself, and a blind substitution would produce a README claiming the clone is a starter to be cloned.

The later `just description` recipe is deliberately narrower. It changes only the paragraph between the repository description markers, along with the matching package and GitHub metadata.

## 2026-07-27: `cargo-audit` alongside `cargo-deny`

`cargo deny check advisories` reads the same RustSec advisory database that `cargo-audit` does. Running both means cloning that database twice per audit for one signal, and two tools that can disagree only through version skew in their copies of the same data. `cargo-deny` stays because it also covers licenses, banned crates, and the source allowlist, which `cargo-audit` does not.

## 2026-07-27: A `cargo test --doc` step in CI

Proposed to sit next to nextest, since nextest deliberately does not run doctests. Checked before writing it: `cargo test --doc` on this crate fails with `error: no library targets found in package cli-template`. It is a binary-only crate, so the step would not run zero tests, it would break the pipeline on the first push.

Recorded as a note in `development.md` instead: a clone that grows a `src/lib.rs` should add `cargo test --doc --locked` at that point.

## 2026-07-27: `cargo-machete` in place of `cargo-shear`

Both find dependencies declared in `Cargo.toml` and never used, and only one belongs in the hook. `cargo-shear` won because it resolves `use` items rather than pattern-matching source text, so it is the one less likely to produce a false positive that teaches you to ignore the hook. No reason to run both.

## 2026-07-27: `cargo-fuzz`

Fuzzing pays for itself against a parser or a decoder handling untrusted bytes. This template reads a file and counts lines. It also requires a nightly toolchain, which contradicts the `channel = "stable"` pin every clone inherits, so shipping it would mean shipping a recipe most clones cannot run.

## 2026-07-27: `cargo-generate` as the rename mechanism

Would turn the repo into a parameterized template with `{{project-name}}` placeholders. Rejected because it stops the template from being a working crate: you could no longer clone it, run `just check`, and see it pass, which is how you know the starter is not broken. It also conflicts with the documented `gh repo create --template` flow. `just rename NEW` gets the same result while the repo stays buildable at every point.

## 2026-07-27: `cargo-expand`, `cargo-cache`, `cargo-edit`, and `cargo-binstall` as template content

All four are useful and none of them are properties of this project. They are tools you install once on a machine and use across every crate you own, so putting them in `setup.md` as project prerequisites would misrepresent what the template actually needs. `cargo-binstall` appears in the install instructions only as the verb that installs the tools that are prerequisites.

## 2026-07-27: Coverage, mutation testing, and binary-size analysis as CI gates

`cargo-llvm-cov`, `cargo-mutants`, and `cargo-bsize` are in the `justfile` and deliberately out of `just ci` and out of the workflow. Each answers a question you ask occasionally, and none has a threshold that is meaningful to fail a merge on: a coverage percentage gate rewards tests that execute lines without asserting on them, and mutation testing the whole crate takes long enough that people start skipping the pipeline. `just mutants` uses `--in-diff` for the same reason.

No Codecov or coverage-upload step either: it needs an account and a token per clone, to display a number nobody is gating on.

## 2026-07-27: Running `cargo-msrv` in CI

The MSRV job installs the floor toolchain and runs `cargo check` instead. That is the actual contract being tested, and it needs no extra tool on the runner. `cargo-msrv` stays a local recipe for the different question of *finding* the floor, which involves compiling repeatedly across a version range and does not belong in a per-push pipeline.

## 2026-07-27: A Dependabot configuration

Considered for keeping dependencies and action pins current in clones. Rejected for the same reason as hash-pinning actions: it is standing machinery imposed on every clone, generating PRs in repositories nobody is watching. Dependency freshness is available on demand through `just outdated`, and the lockfile plus `cargo-deny` advisories cover the case that actually matters, which is a known vulnerability rather than a stale minor version.

(The template repo itself runs Renovate to keep the *template's* dependencies current; clones do not inherit it, because the Renovate app is not enabled on them.)

## 2026-07-27: Limiting `--locked` to CI

The first sketch put `--locked` only in the workflow, to spare the local loop a failure when the lockfile is momentarily behind the manifest. Rejected because it recreates the exact split this pass set out to close: a flag that changes what "green" means, present in one place and absent in the other. `--locked` is in `just check`, the prek clippy hook, and CI, so a lockfile that drifted from the manifest fails on the machine where it drifted.

## 2026-07-27: A module-level `#![allow(clippy::print_stderr)]` in `ui.rs`

`print_stderr` is `deny` crate-wide, and `ui.rs` is the module whose job is writing to stderr, so an allow with a comment looked like the honest exemption. Unnecessary in the end: `ui.rs` imports `anstream::eprintln`, and the lint fires on the std macros, so the deny holds everywhere with no exemption at all. The version that needs no escape hatch is better than the one that documents its escape hatch well.

## 2026-07-27: cargo-dist

cargo-dist generates installer scripts, a build matrix, and the release workflow from one `dist-workspace.toml`. A survey of ripgrep, fd, bat, delta, hyperfine, mise, and watchexec found exactly one user: uv, which ships to millions of installs across 18 targets. watchexec adopted cargo-dist and later dropped it for a hand-rolled tag-triggered workflow, which is what ripgrep, fd, bat, and delta all run as well. The step a clone of this template actually needs is that ripgrep-style `release.yml`, one paragraph in `release.md`, not generator machinery every clone would have to delete before its first release.

## 2026-07-27: tracing (and tracing-subscriber)

The structured-logging stack pays off in async or long-running processes, which is exactly where its users in the survey live (jj, uv, watchexec). A small synchronous CLI has one thread and one phase per command; the `Verbosity` enum in `output.rs` is the whole state machine it needs. tracing's default format also writes log-level labels (INFO, WARN) to stderr, which the contract's narration style deliberately avoids in normal mode. If a clone grows async work or a daemon mode, this becomes the right answer, and `clap-verbosity-flag` maps the flags to its levels.

## 2026-07-27: clap-verbosity-flag

A crate whose entire job is the `-q` / `-v` counting that `cli.rs` and `output.rs` already do in about fifteen lines, with the exact semantics the contract wants (`-v` is never version, quiet conflicts with verbose). Shipping it would be a dependency standing in for code shorter than its own documentation. It becomes the right adopt candidate only alongside tracing, for a clone that needs real leveled logging.

## 2026-07-27: concolor and supports-color

Terminal color-detection crates. `anstream` already resolves auto/always/never plus NO_COLOR and TERM=dumb, and it is the implementation clap itself uses, so a second detector could only disagree with the first. concolor specifically is unmaintained (last commit 2023-04), and supports-color, though alive, would be redundant the day it was added.

## 2026-07-27: jemalloc

fd's allocator swap is the standard trick for allocation-heavy CLIs, with a platform carve-out for macOS. A starter has no measured allocation problem to fix, and a global allocator is an invisible default a clone should choose deliberately after profiling, not inherit. The prerequisite is a benchmark and `just bloat` output pointing at allocation, not a dependency line.

## 2026-07-27: `debug = 1` in the release profile

jj and watchexec keep line tables in their stripped release binaries so user panic reports carry usable stack traces. Here `panic` is denied in production code, so the only traces this buys are for dependency bugs, and the cost is binary size on every release build of every clone. A clone that starts shipping to users who file crash reports can revisit with a real need.

## 2026-07-27: Rendering the man page at build time into a shipped file

Generating a man page is cheap; shipping one as a build artifact is not. A `build.rs` writing roff into `OUT_DIR` gives the clone a build dependency and a lookup to understand, and without binary distribution there is no install path for the resulting file to land in.

The template takes the other route and keeps the feature: `cli-template man` renders the page on demand to stdout, like every other piece of data. It cannot drift from the flags, it needs no build script, and a clone that later adds distribution pipes it into a file at install time. See `docs/contract.md` for the shape.

## 2026-07-27: Self-update and update notifications

mise's `self_update` feature and starship-style update notices both presume published releases and a user base to notify. A template has neither, and the mechanism depends on where the clone publishes (crates.io, GitHub releases, a tap), which is the clone's decision to make at that point, not one to inherit silently.

## 2026-07-27: A markdown linter (rumdl) on the commit hook

Tempting here specifically, because `docs/` is a large share of what this repo is, and the argument that earned `typos` its place (prose that ships to users deserves a check) reads as though it applies with more force to seven doc files.

It does not, because the two differ in who owns the prose. The doc comments `typos` guards are `src/cli.rs`'s `--help` text: code, shipped to the clone's users, and inherited verbatim. The markdown is documentation *about the template*, most of which a clone rewrites or deletes. Adding a hook plus a checked-in `.rumdl.toml` would impose the template author's markdown style on every clone's own README, which is the Dependabot objection above in a different costume: standing machinery in repositories nobody is watching.

Run it by hand on this repo if you like; `.gitignore` already covers its cache. The decision is only that it is not a gate a clone inherits.

## 2026-07-27: A checked-in CLI spec (usage-lib, mise.usage.kdl)

mise generates completions, man pages, and docs from one KDL spec instead of clap derive. That trades the derive API the whole ecosystem documents for a second definition of the CLI the clone has to learn, and it makes `src/cli.rs` stop being the source of truth. The template's bet is the opposite: the doc comments in `cli.rs` are the help text, one source, no generation step.

## 2026-08-25: Restoring the terminal on SIGTERM and SIGHUP

The TUI template converts `SIGTERM` and `SIGHUP` into a quit request the event loop checks, so a kill while the alternate screen is up still runs the terminal restore. A CLI has no alternate screen and never enters raw mode, so there is nothing to restore: a kill mid-command leaves the terminal exactly as it was, and the progress layer never hides the cursor (checked against the vendored indicatif source). Recorded so the question is settled before it is re-asked.

## 2026-09-04: `clippy::nursery` as a lint group

Denying `pedantic` and `nursery` together is the headline of No Boilerplate's 2026 Rust toolkit config. `pedantic` is already on here; `nursery` is not, and running it produced eight findings that between them make the case against it. Six are `missing_const_for_fn` on the `palette.rs` accessors. One is `option_if_let_else` proposing a `map_or_else` chain in `paths.rs` that reads worse than the `if let` it would replace. The last is `literal_string_with_formatting_args` firing on `"  {msg} [{bar:30}] {pos}/{len}"` in `ui.rs`, which is an indicatif template rather than a format string and cannot be written any other way.

That last one is the disqualifier, and it is the `unreachable_pub` objection above in a different costume: a lint that fires on the only correct form trains people to ignore lint output. nursery lints are also still under evaluation upstream by definition, so a clone would inherit churn from a set whose contents move without a major version.

## 2026-09-04: `clippy::as_conversions`

From the same config, and the only lint in it that fires on production code here: `checks.len() as u64` in `doctor.rs`, feeding indicatif's `ProgressBar::new`.

Rejected because `pedantic` already denies the casts that lose data (`cast_possible_truncation`, `cast_possible_wrap`, `cast_sign_loss`, `cast_precision_loss`) and stays quiet on this one deliberately, since `usize` is at most 64 bits on every Rust target and the cast cannot truncate. Satisfying `as_conversions` would mean `u64::try_from(checks.len()).unwrap_or(u64::MAX)`, a fallback branch no input can reach. A lint whose only effect on this codebase is to add unreachable defensive code is the worse trade.

## 2026-09-04: `clippy::string_slice`

Also from that config. Its only hit is `stderr[cause..]` in `tests/cli.rs`, sliced at a byte offset `str::find` returned and therefore always on a character boundary.

Rejected on consistency. `clippy.toml` deliberately reopens `unwrap`, `expect`, `panic`, and now indexing inside tests, because a panic in a test is the assertion. `string_slice` has no `allow-...-in-tests` key, so denying it would impose on test code a rule this repo has decided not to impose, in exchange for no production coverage at all: nothing in `src/` slices a string.

## 2026-09-04: A nightly toolchain

The video has been on nightly since 2020 and leads with compile times and the new trait solver. Rejected for the reason `cargo-fuzz` was: `channel = "stable"` in `rust-toolchain.toml` is inherited by every clone, and shipping a default most clones should not be on is not a starting point.

There is a second reason specific to this template. The lint set *is* the contract, and nightly clippy knows lints stable does not, so `just check` would mean something different depending on who ran it. That is the same disagreement `rust-toolchain.toml` exists to prevent.

## 2026-09-04: eyre and color-eyre in place of anyhow

Rejected on the same ground as `miette` above: it is a second error-rendering system standing next to the `Error:` / `Try:` / `Cause:` printer in `main.rs` that already satisfies the contract, and the contract is about the shape of the message rather than its decoration.

There is also a specific mismatch. What `color-eyre` leads with is a panic and backtrace hook, and `panic` is denied crate-wide, so the headline feature has nothing to hook. `anyhow` stays for the further reason that `thiserror` is already the typed half of the pair, and the two share an author and an idiom.

## 2026-09-04: bacon or watchexec as a `just watch` recipe

The video pairs `bacon clippy` with rust-analyzer as the inner loop, and it is a good loop. Rejected as template content for the reason `cargo-expand` and `cargo-edit` were: these are tools you install once per machine and use across every crate you own, so a recipe would put a prerequisite in the `justfile` that `just check` does not need and a fresh clone would not have. Nothing stops running `bacon clippy` in a second pane; it just is not a property of this project.

## 2026-09-04: The application crates from the same toolkit

itertools, rayon, criterion, jiff, reqwest, sqlx, utoipa, leptos, and dioxus are each defensible in the project that needs them and each a deletion in the project that does not. `serde` is here because JSON output is a contract point; none of these has a contract point to hang on. Recorded so the list is settled rather than re-litigated: a clone adds what its domain needs, and the template's job is to have not guessed wrong on its behalf.

Two notes worth keeping. **jiff** has displaced chrono as the answer for a clone that needs time handling. **criterion** is not blocked by the CodSpeed entry above, which was about a workflow pointing at a `benches/` directory that does not exist; a clone that grows a real benchmark adds criterion at that point.

**devenv**, which the video manages the entire toolchain with, is out for a plainer reason: `rust-toolchain.toml` and the `justfile` already pin what this repo needs, and a Nix-based environment is a machine-level choice a template cannot make on behalf of its clones.


## 2026-09-04: `cargo-hack`, `taplo`, and a rustdoc job

Found by the same fourteen-repo survey as the hash-pinning re-check above, and turned down as the tail of it.

`cargo-hack` checks the feature powerset. One repo of the fourteen uses it (eza), and this crate has no optional features for it to combine, so it would iterate over a set of one. It becomes worth adding the day the manifest grows a `[features]` table.

`taplo` formats and lints TOML. Also one of fourteen (starship). The manifest, `deny.toml`, `clippy.toml` and `release-plz.toml` here are hand-maintained and small enough that a formatter would only ever reformat what a human already read.

A rustdoc job (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`) is the closest call of the three: five of fourteen run one, including ripgrep, jj, bat and delta, and `missing_docs` is already `warn` here so doc comments are not decorative. Left out because what it would add over the existing gate is broken intra-doc links, and this is a binary crate whose doc comments are read as `--help` text rather than as rendered rustdoc. A clone that grows a `src/lib.rs` should add it in the same breath as the `cargo test --doc` step noted in `development.md`.

## 2026-10-06: Sources considered and not built

Each was weighed in the source research that produced ADR 0004.

- **Papers with Code**: shut down in July 2025; every URL, the API included, redirects to Hugging Face trending papers.
- **data.world**: search answers 401 without a key and the catalog is commercial; nothing it holds was missing from the open sources.
- **ICPSR, GESIS, UK Data Service, USGS ScienceBase direct**: each served a Cloudflare challenge page to non-browser clients. Their metadata arrives through DataCite and the CESSDA catalogue instead.
- **Dryad direct**: a clean keyless API, but DataCite carries every Dryad DOI; revisit only if file-level details are wanted.
- **Cloud marketplaces** (BigQuery public datasets, Snowflake, Databricks, AWS Data Exchange): listing needs cloud credentials and an account.
- **LDC**: a paywalled membership catalog.
- **Internet Archive**: a `mediatype:data` query returned books; too noisy to merge.
- **Academic Torrents**: asks clients to read its RSS feeds rather than search; small.
- **re3data**: a registry of 3,534 repositories, not of datasets; useful for pointing at repositories, not for search.
- **Materials Project core API**: a per-material database rather than a dataset catalog; its contributed datasets (MPContribs) are searched instead.

## 2026-10-07: `panic = "abort"` in the release profile

It would save 864 KiB of the 4.5 MiB shipped at v0.6.0, the second-largest stable lever ([measurement](../bench/2026-10-07-binary-size.md)). Every source runs on its own thread in `search.rs`, and a panic in a dependency unwinds only that thread, so the search reports the source as crashed and still prints the others. Under abort, one malformed response that trips a panic in quick-xml or serde_json ends the whole search with no results. Destructors would also be skipped: a cache write in progress would leave its temp file behind (the target file stays intact, because the rename is atomic) and the progress line would be left half drawn. Tests would not notice, because cargo builds tests with unwinding regardless. Revisit only if adapters stop running on threads that are allowed to fail.

## 2026-10-07: `opt-level = "z"` or `"s"` for dataseek's own code

Building the crate itself for size takes the release binary from 3.79 MB to 2.97 MB on aarch64 macOS, and makes every CPU stage of a search 40% to 300% slower ([measurement](../bench/2026-10-07-binary-size.md)). Dependencies are built at `z` instead, because fat LTO inlines their hot paths into this crate's code, which stays at 3.
