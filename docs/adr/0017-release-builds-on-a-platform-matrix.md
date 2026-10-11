# ADR 0017: Release builds run on a per-platform matrix

- Status: accepted
- Date: 2026-10-07

## Context

PyPI wheels and `cargo binstall` both need prebuilt binaries for Linux (x86_64, aarch64), macOS (aarch64, x86_64) and Windows (x86_64). ADR 0002 keeps CI to one Linux job, which cross-checks Windows and macOS with clippy but cannot produce their binaries.

## Decision

`.github/workflows/release.yml` runs when a GitHub release is published, which release-plz does when its release PR merges. One job per target builds the wheel with maturin, on a machine of the target's OS (Linux aarch64 in maturin's cross container), and archives the same binaries as `dataseek-<target>.tar.gz` (`.zip` on Windows) with a SHA-256 file. The archives are attested and attached to the release; the wheels and an sdist go to PyPI with `uv publish` through trusted publishing. A pull request that changes the release build runs the builds and publishes nothing. CI stays one Linux job.

## Consequences

- Each release starts eight machines, two of them macOS. They run once per version, never per push.
- A build that fails fails only this workflow. The tag and GitHub release already exist, so the fix is a rerun or a patch release, never a retag.
- macOS and Windows binaries are built only at release and on pull requests that change the release build; the clippy cross-checks in CI still catch compile errors there.

## Evidence

`maturin build --release --locked --target aarch64-apple-darwin` packs both `dataseek` and `dsk` into one `py3-none-macosx_11_0_arm64` wheel, `uvx --from <wheel> dsk --version` runs it, and the workflow's archive step, run locally on that build, produces `dataseek-aarch64-apple-darwin/{dataseek,dsk}` with its checksum (2026-10-07).
