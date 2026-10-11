# ADR 0010: `dsk` is the same program under a short name

- Status: accepted
- Date: 2026-10-06

## Context

`dataseek` is long to type for a tool used many times a day. A shell alias would need setting up per shell and would not get completions.

## Decision

The crate is a library plus two binaries: `src/main.rs` builds `dataseek`, `src/bin/dsk.rs` builds `dsk`, and both call `dataseek::main()`. clap takes the usage name from `argv[0]`, so `dsk -h` says `Usage: dsk`, and `completion` writes a script for whichever name ran it. `--version` prints the package name, `dataseek`, for both.

## Consequences

- `cargo install --path .` installs both names. Every flag, output and exit code is identical.
- The library exists only to share the entry point; every module stays private, so it is not a public API.
- A library target means doctests exist; CI runs `cargo test --doc` next to nextest.
