# ADR 0002: CI is one Linux job that cross-checks Windows and macOS

- Status: accepted
- Date: 2026-09-25

## Context

CI ran a three-OS matrix (ubuntu, macOS, Windows). It guarded two things: path layouts, which `etcetera` resolves as XDG on every Unix and differently only on Windows, and `cfg` boundaries that fail to compile off Linux. On a private repository a macOS runner bills at ten times the Linux rate and Windows at twice.

## Decision

`.github/workflows/ci.yml` is one `ubuntu-latest` job, `linux`, with every check as a named step. The `cross-check` step runs clippy for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` from that machine; `just cross` runs the same commands locally. The `msrv` step runs last because it installs a second toolchain, and every earlier step failing first is cheaper.

## Consequences

- Compile boundaries on Windows and macOS stay covered; runtime behavior there (path resolution, terminal detection) is not exercised.
- `linux` is the single check the ruleset requires.
- Shipping binaries for Windows or macOS adds a per-OS build matrix in the release workflow, gated on a release having happened. That matrix, not this job, is where runtime coverage on those platforms would return.

## Evidence

The bug the matrix once caught, an unconditional `SIGPIPE` import from `signal-hook`, was a compile failure on Windows; clippy for the Windows target reports it from Linux.
