# ADR 0012: Benchmarks run as Criterion locally and as CodSpeed's CPU simulation in CI

- Status: accepted
- Date: 2026-10-06

## Context

A search spends seconds on the network and milliseconds on the CPU, so a CPU regression is invisible in `dataseek bench` until it is large. The CPU work that grows with input is in five places: parsing a downloaded catalog (SDMX-ML, Eurostat's table of contents), loading a cached catalog from JSON, searching it locally, fusing the per-source lists, and cleaning remote text. Wall-clock numbers from GitHub's shared runners are too noisy to compare across commits, and this private repository pays for runner minutes (ADR 0002).

## Decision

`benches/search.rs` is one Criterion target with a group per stage, each at three geometrically spaced sizes. The harness is `codspeed-criterion-compat` renamed to `criterion`, which is plain Criterion under `cargo bench` and CodSpeed's instrumentation under `cargo codspeed`.

- Locally, `just bench` measures wall time; `--save-baseline` and `--baseline` compare two states of the tree.
- In CI, `.github/workflows/codspeed.yml` runs the same benches once under CodSpeed's CPU simulation on `ubuntu-latest`. The simulation counts instructions and models the cache, so a pull request is compared with its base on main without runner noise. CodSpeed's own pull request check reports regressions; Criterion never gates, because it exits 0 on a regression.
- The library stays private. The `internals` feature re-exports exactly the benchmarked functions under `dataseek::internals`, and the public-API lints (`missing_docs`, `must_use_candidate`, `return_self_not_must_use`, `missing_errors_doc`) are allowed only while that feature is on, since none of them can fire in the default build.

## Consequences

- CI gains a second workflow: one job, Linux only, OIDC upload, no secret. Pull requests trigger it only when `src/`, `benches/` or the manifests change; every push to main runs it so each commit there is a baseline. This is the one exception to ADR 0002's single job, kept apart so its path filter does not touch the required `linux` check.
- CodSpeed is an external service: the repository has to stay imported at codspeed.io, on the free plan (CPU simulation, up to five users).
- `cargo-codspeed` in the workflow is pinned to the version of the `codspeed` crate in `Cargo.lock`; bumping one without the other breaks the job.
- The per-adapter JSON mappers are not benchmarked: each reads at most one API page, microseconds next to its request. An adapter that pages through results, or a profile that shows a mapper hot, is the trigger to add one.
- `cargo codspeed run` measures only on Linux; on a Mac, the local numbers are Criterion's wall time.

## Evidence

- 2026-10-06: the `codspeed` crate's build script compiles C through `cc` but falls back to a no-op when that fails, and `nix` compiles for the Windows target, so `just cross` (clippy for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` with `--all-targets --all-features`) still passes with the bench target in the graph.
- 2026-10-06: `just bench --test` ran all 18 benchmarks once; baseline numbers are in [`../bench/`](../bench/).
