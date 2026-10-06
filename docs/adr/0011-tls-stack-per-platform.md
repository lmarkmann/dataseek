# ADR 0011: The TLS stack is chosen per platform

- Status: accepted
- Date: 2026-10-06

## Context

ureq's default TLS is rustls with ring. ring compiles C against the target's system headers, and CI is one Linux job that cross-checks Windows and macOS with clippy (ADR 0002): the cross-check failed with `fatal error: 'assert.h' file not found` for `x86_64-pc-windows-msvc`, and the same holds for the Apple target.

## Decision

`Cargo.toml` enables ureq per target: `native-tls` (SChannel on Windows, Security.framework on macOS, both Rust bindings with no C build) on Windows and macOS, the default rustls everywhere else. `src/http.rs` selects the matching provider with the same `cfg`, and on the native targets verifies against the platform trust store.

## Consequences

- Windows and macOS users get their OS trust store, so a corporate root CA works there without configuration; Linux users get Mozilla's bundled roots.
- Behavior can differ by platform at the TLS layer. Known case: `cn.dataone.org` renegotiates TLS, which rustls refuses and Security.framework accepts; the DataONE adapter uses `search.dataone.org`, which works on both.
- Reverting to rustls everywhere breaks the cross-check, or requires dropping it from CI, which reverses ADR 0002.

## Evidence

- 2026-10-06: `cargo clippy --target x86_64-pc-windows-msvc` failed in ring's build script with rustls; with the split, both cross-targets pass clippy and a live search on macOS answered from six sources over native TLS.
