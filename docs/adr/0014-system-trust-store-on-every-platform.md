# ADR 0014: Every platform trusts the system's root certificates

- Status: accepted; supersedes [ADR 0011](0011-tls-stack-per-platform.md)
- Date: 2026-10-06

## Context

ADR 0011 split the TLS stack by platform so CI's cross-checks compile: native-tls on Windows and macOS, rustls everywhere else. The split also split the trust: Windows and macOS verified against the OS's store, Linux against Mozilla's roots compiled into the binary. Behind a TLS-inspecting proxy, which re-signs every connection under a root certificate the organisation installs on each machine, Linux therefore reached no source at all, while the same network worked on the other two platforms.

## Decision

The TLS stack stays split as ADR 0011 describes. The roots do not: `src/http.rs` asks ureq for `RootCerts::PlatformVerifier` on every platform, and on Linux `Cargo.toml` enables ureq's `platform-verifier` feature, which verifies through rustls-platform-verifier against the certificates rustls-native-certs loads from the system (`/etc/ssl/certs` and the distribution's equivalents).

## Consequences

- A root installed with the distribution's tool (`update-ca-certificates`, `update-ca-trust`) is trusted on Linux as it already was on Windows and macOS.
- On Linux, `SSL_CERT_FILE` and `SSL_CERT_DIR` replace the system store when set, as they do for OpenSSL; `dataseek help environment` lists them.
- A Linux system with no CA bundle at all, such as a bare container image, now reaches no source: every request fails with "No CA certificates were loaded from the system", which `-v` shows per source. Installing the distribution's `ca-certificates` package fixes it. Before, the bundled roots hid the gap.
- The binary no longer follows Mozilla's root program on its own schedule; on Linux it trusts whatever the system's store says, as on the other platforms.
- Reverting means the bundled roots again, and a proxy's root again unreachable on Linux.

## Evidence

- 2026-10-06: `linux_trusts_the_root_certificates_the_system_names` in `tests/cli.rs` serves a page over HTTPS under a root generated for the test and points `SSL_CERT_FILE` at it. With the bundled roots, `inspect` failed with an unknown-issuer error; with the platform verifier it read the page. It ran both ways in a Linux container before the change merged, and runs on every CI build.
