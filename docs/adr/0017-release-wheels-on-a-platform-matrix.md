# ADR 0017: Release wheels build on a per-platform matrix

- Status: accepted
- Date: 2026-10-07

## Context

The PyPI package is the compiled binaries in a wheel, one wheel per platform: Linux (x86_64, aarch64), macOS (aarch64, x86_64) and Windows (x86_64). ADR 0002 keeps CI to one Linux job, which cross-checks Windows and macOS with clippy but cannot produce their binaries.

## Decision

`.github/workflows/release.yml` runs when a GitHub release is published, which release-plz does when its release PR merges. It builds one wheel per target with maturin, on a machine of the target's OS (the Linux aarch64 wheel in maturin's cross container), plus an sdist, and its `pypi` job uploads them with `uv publish` through PyPI trusted publishing. CI stays one Linux job.

## Consequences

- Each release starts six machines, two of them macOS. They run once per version, never per push.
- A wheel that fails to build fails only this workflow. The tag and GitHub release already exist, so the fix is a rerun or a patch release, never a retag.
- macOS and Windows binaries are first built at release, not on a pull request; the clippy cross-checks in CI still catch compile errors there.

## Evidence

`maturin build --release --locked` on aarch64 macOS packs both `dataseek` and `dsk` into one `py3-none-macosx_11_0_arm64` wheel, and `uvx --from <wheel> dsk --version` runs it (2026-10-07).
