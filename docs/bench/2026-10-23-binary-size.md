# Release binary size, 2026-10-23

## Question

Which dependencies cost more bytes than the part of them dataseek uses, and what does replacing or trimming each one save, without slowing a search? Primary metric: the file size of the stripped release `dataseek` binary, which is what a download costs. Second metric: the size of the release archive. Speed must not regress: `just relevance`, `just bench-startup`, the replaced code timed against what it replaced, and CodSpeed on the pull request.

## Machine

```text
date                 2026-10-23 UTC
os                   macOS 27.0.1 arm64
cpu                  Apple M4
performance_cores    4
efficiency_cores     6
memory_gib           16
power                mains, battery charged
rustc                1.99.1
llvm                 23.1.1
host                 aarch64-apple-darwin
cross builds         cargo-zigbuild with zig 0.17.0
size tool            cargo-bsize 0.0.2
baseline commit      2f5c27c (v0.10.0)
```

## Setup

- Every build ran under `flock /tmp/heavy-build.lock`, so nothing else compiled while a size or a time was taken.
- `aarch64-apple-darwin` was built with `cargo build --release`, `x86_64-unknown-linux-gnu` with `cargo zigbuild --release`. Each commit of the branch was built on its own, so each row's change is that commit alone.
- Candidates were first measured as stubs: the dependency's call replaced by the cheapest stand-in, which bounds what a rewrite can save. Only candidates whose bound was worth a rewrite were rewritten, and the rewrite was measured again.
- Clean build times were taken with `RUSTC_WRAPPER=` so sccache could not serve cached crates, twice per commit.

## Results

File size of the stripped release binary, in bytes, each commit on top of the one before.

| commit | `aarch64-apple-darwin` | change | `x86_64-unknown-linux-gnu` | change |
|---|---:|---:|---:|---:|
| v0.10.0 | 2,764,128 | | 4,351,152 | |
| ureq 3.5 without the Mozilla root store | 2,648,672 | -115,456 | 4,296,232 | -54,920 |
| Markdown stripped without pulldown-cmark | 2,466,608 | -182,064 | 3,983,720 | -312,512 |
| `credentials.toml` read without toml | 2,399,952 | -66,656 | 3,885,536 | -98,184 |
| progress drawn without indicatif | 2,316,560 | -83,392 | 3,797,176 | -88,360 |
| man page written without clap_mangen | 2,283,248 | -33,312 | 3,758,392 | -38,784 |
| total | | -480,880 (-17.4%) | | -592,760 (-13.6%) |

Release archive of the final build, `dataseek` and `dsk` inside:

| archive | `aarch64-apple-darwin` | `x86_64-unknown-linux-gnu` |
|---|---:|---:|
| `.tar.gz`, as v0.10.0 packed it | 2,363,031 | 3,689,377 |
| `.tar.xz` | 863,412 | 1,493,620 |

gzip's 32 KB window never sees that `dsk` repeats `dataseek`, so a `.tar.gz` holds two compressed copies. xz's window spans both, so `dsk` costs a few hundred bytes. The `.tar.xz` is 63% (macOS) and 60% (Linux) smaller than a `.tar.gz` of the same binaries; against v0.10.0's Linux archive, 4,059,170 bytes, the download is 63% smaller.

### What each change replaced

- **ureq 3.5.** `native-tls-no-default` now builds a working connector (ureq PR #1204), so macOS and Windows stop linking `webpki-root-certs`, which `RootCerts::PlatformVerifier` never reads. Linux moved to `rustls-no-provider` with ring's provider passed in `src/http.rs`, which drops the same roots there. A live `dataseek search` over each TLS stack is the test, because no test here makes a real TLS connection.
- **pulldown-cmark.** Descriptions only need Markdown turned into plain text, not parsed into a tree. `record::from_markdown` strips fences, rules, link definitions, list and quote markers and headings line by line, then pairs code spans, brackets and emphasis in one linear, non-recursive pass. Over every string in the recorded fixtures, 4,693 of 4,697 come out the same as before; of the four that differ, one reads better (`**Title:**Plasma`) and three are an opendatasoft field that never goes through Markdown. A test feeds it 50,000 nested openers of every kind and asserts linear time. Over the same strings, `clean(from_markdown(..))`, which is what a record's description goes through, takes 56 ms per 20 passes against 62 ms with pulldown-cmark, best of 7, and 3.65 ms against 4.41 ms on a 20 KB synthetic description. The new `markdown` bench group keeps it measured. The first version was twice as slow as pulldown-cmark, because it matched every line for code, links and emphasis and copied one character at a time; it now skips a match whose opening byte the line lacks and copies plain runs whole.
- **toml.** `credentials.toml` holds one `name = "value"` per line. `credentials::flat_table` reads exactly that, with TOML's string escapes, and treats anything else (a table, a multi-line string, a duplicate key) as supplying no key at all.
- **indicatif.** dataseek draws one spinner or one bar at a time. `ui::Progress` is a ticker thread redrawing one line every 90 ms and clearing it when the work finishes or the handle drops.
- **clap_mangen.** The page has one fixed shape: options, commands, examples, exit status. `help::man_page` writes that roff from the clap definition. It also stopped referring to `dataseek-search(1)` and the other per-command pages, which were never installed.

### Measured and not adopted

On `aarch64-apple-darwin` unless the row says otherwise; the base is the binary before the man page change, 2,316,560 bytes.

| lever | change | why not |
|---|---:|---|
| clap without `suggestions` | -16,560 | a mistyped flag loses its `a similar argument exists` tip |
| clap without `wrap_help` | -48 | nothing to gain |
| clap without `error-context` | 0 | nothing to gain |
| ASCII-only uppercasing in `record::file_format` | -16 | the 25 KiB case table bsize flags is the lowercase one, which matching needs |
| Linux: ring provider cut to AES-GCM suites and X25519, P-256 | +384 | ring links every cipher either way |
| Linux: `--icf=all` | | zig's linker rejects it; fat LTO with one codegen unit already merges identical functions |
| `panic = "abort"`, on v0.10.0 | -413,936 | a panicking adapter would end the whole search instead of failing one source |
| replacing anyhow | | 5.3 KiB of crate code; a boxed error with a context wrapper adds back most of it |

### Why Linux stays larger

On macOS and Windows the TLS stack is the operating system's, linked dynamically. On Linux it is in the binary: rustls (306 KiB), ring's C and assembly (316 KiB, of which `ecp_nistz256_precomputed`, the P-256 table, is 148 KiB), ring's Rust (90 KiB) and webpki (53 KiB), about 760 KiB together. A position-independent ELF adds 112 KiB of `.rela.dyn` relocation records, x86_64 code is less dense than arm64, and its unwind tables are larger. Linking the system OpenSSL instead is not an option for the manylinux wheels the Linux binaries ship in: auditwheel would vendor libssl into the wheel.

### Compile time

Clean builds of `dataseek` with sccache off (`RUSTC_WRAPPER=`) and eight jobs, twice each:

| build | v0.10.0 | after | crates linked on macOS |
|---|---:|---:|---:|
| release | 30 s | 27 s | 95 -> 79 |
| dev | 7 s | 6 to 7 s | |

The release build spends most of its time in one fat-LTO codegen unit, so sixteen fewer crates save three seconds. A faster linker would save less: on Linux, rustc already links with rust-lld, and mold or wild cut a link that is a few percent of a release build to about one percent. Neither runs on macOS.

## Verdict

Five dependencies were each replaced by the few dozen to few hundred lines dataseek needed of them: the binary is 17.4% smaller on macOS and 13.6% on Linux, and the archive a user downloads is about 60% smaller. Every test passes and `just relevance` is unchanged at nDCG@10 0.6403. The largest lever left is `panic = "abort"`, kept off because one adapter's panic would end the whole search.

## Caveats

- A live search over native-tls on macOS answered from 70 of 71 sources. The Linux TLS change (rustls without a bundled provider, ring's passed in) was not tried live: no Linux machine or container was available, and CI's Linux job builds and tests it without a TLS connection.
- The Linux binary was cross-built with zig as the linker; the release job builds natively. Its relative changes are the result, its absolute size is not.
- Windows was not built. Its TLS stack and dependencies follow macOS, so its changes should too.
- The stub bounds overstate what a rewrite saves, because the rewrite's own code comes back: pulldown-cmark's stub saved 199 KB on macOS, the rewrite 182 KB.
