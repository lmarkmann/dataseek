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
size tool            cargo-bsize 0.0.2, bloaty
nightly              rustc 1.101.0-nightly (2026-10-20), for the #[optimize] rows
containers           colima 0.10.3, Docker Engine 29.5.2, debian:trixie
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
| Windows console check, rustls-webpki 0.103.16 | 2,283,248 | 0 | 3,763,816 | +5,424 |
| review fixes | 2,299,744 | +16,496 | 3,769,912 | +6,096 |
| total | | -464,384 (-16.8%) | | -581,240 (-13.4%) |

The last two rows came after the size work. The first is the progress line's check that a Windows console understands escape codes (1,200 bytes on Linux) and the rustls-webpki release that distrusts the emSign intermediates (4,224 bytes). The second is what review of the rewrites found: `credentials.toml` with a byte order mark or TOML 1.1's `\e` and `\x` escapes read again, Markdown that the first rewrite read differently from pulldown-cmark (a wrapped line starting `2020.`, a `[label]: text` line, a line opening with a code span), Greek and math entity names, the progress bar dropped on a narrow terminal, and `enable-ansi-support` in place of `anstyle-query`, which gave up when stdout was redirected. Its code is 5.2 KB on macOS; the rest of the 16 KB is `__TEXT` crossing a 16 KiB page.

Release archive of the build before the review fixes, `dataseek` and `dsk` inside:

| archive | `aarch64-apple-darwin` | `x86_64-unknown-linux-gnu` |
|---|---:|---:|
| `.tar.gz`, as v0.10.0 packed it | 2,363,031 | 3,689,377 |
| `.tar.xz` | 863,412 | 1,493,620 |

gzip's 32 KB window never sees that `dsk` repeats `dataseek`, so a `.tar.gz` holds two compressed copies. xz's window spans both, so `dsk` costs a few hundred bytes. The `.tar.xz` is 63% (macOS) and 60% (Linux) smaller than a `.tar.gz` of the same binaries; against v0.10.0's Linux archive, 4,059,170 bytes, the download is 63% smaller.

### What each change replaced

- **ureq 3.5.** `native-tls-no-default` now builds a working connector (ureq PR #1204), so macOS and Windows stop linking `webpki-root-certs`, which `RootCerts::PlatformVerifier` never reads. Linux moved to `rustls-no-provider` with ring's provider passed in `src/http.rs`, which drops the same roots there. A live `dataseek search` over each TLS stack is the test, because no test here makes a real TLS connection.
- **pulldown-cmark.** Descriptions only need Markdown turned into plain text, not parsed into a tree. `record::from_markdown` strips fences, rules, link definitions, list and quote markers and headings line by line, then pairs code spans, brackets and emphasis in one non-recursive pass. Over every string in the recorded fixtures, 4,693 of 4,697 come out the same as before; of the four that differ, one reads better (`**Title:**Plasma`) and three are an opendatasoft field that never goes through Markdown. A test feeds it 50,000 nested openers of every kind and asserts it finishes in seconds, which a quadratic scan would not. Over the same strings, `clean(from_markdown(..))`, which is what a record's description goes through, takes 56 ms per 20 passes against 62 ms with pulldown-cmark, best of 7, and 3.65 ms against 4.41 ms on a 20 KB synthetic description. The new `markdown` bench group keeps it measured. The first version was twice as slow as pulldown-cmark, because it matched every line for code, links and emphasis and copied one character at a time; it now skips a match whose opening byte the line lacks and copies plain runs whole.
- **toml.** `credentials.toml` holds one `name = "value"` per line. `credentials::flat_table` reads exactly that, with TOML 1.1's string escapes and a leading byte order mark, and treats anything else (a table, a multi-line string, a duplicate key) as supplying no key at all.
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
| dataseek's own code at `opt-level = "z"`, on the final build | -558,112 | not timed; it can only be slower than the next row |
| the same, with every function in `cache`, `catalog`, `dedup`, `record`, `sdmx` and `eurostat` marked `#[optimize(speed)]` (nightly; stable from Rust 1.101) | -525,456 | parsing, matching and merging 10% to 75% slower |

The last two rows are the largest lever left, and the reason it stays unused. Under fat LTO, dependency code inlined into dataseek is compiled at dataseek's level, so `z` shrinks clap's parser, `core` and the unwind tables along with dataseek's own functions. `#[optimize(speed)]` only covers the body of the function it marks: the serde, iterator, sort and hash code a marked function instantiates for dataseek's types is compiled as separate functions at the crate's `z`. Criterion medians on nightly, the shipped profile run before and after the split to bound noise (the two runs agreed within 1.3%):

| group | split against shipped |
|---|---:|
| `markdown` | +75% |
| `catalog/load` | +60% |
| `parse/eurostat` | +54% |
| `merge` | +32% to +40% |
| `catalog/search` | +31% to +36% |
| `parse/sdmx` | +11% |
| `clean` | +10% |

std rebuilt without its `backtrace` feature (nightly `-Zbuild-std=std,panic_unwind -Zbuild-std-features=panic-unwind`, with `core`, `alloc`, `std` and `hashbrown` held at `opt-level = 3`) is the one lever here that costs no speed: 2,150,560 bytes against 2,283,296 for the same rebuild with the feature, -132,736, and every bench group within 2% of the shipped profile except `catalog/load/4000` at +4%, inside the 10% the two shipped runs differed by. It drops gimli, addr2line and rustc_demangle, which only symbolize a `RUST_BACKTRACE=1` trace; panics still unwind. It needs a nightly toolchain for the release build, so it joins the parked nightly levers in the watchlist.

Keeping the hot modules at `opt-level = 3` on stable would need them in a second crate of the workspace, which would have to be published alongside dataseek for `cargo install` to build it.

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

Four dependencies were replaced by the few dozen to few hundred lines dataseek needed of them, and ureq stopped linking a root store it never read: the binary is 16.8% smaller on macOS and 13.4% on Linux, and the archive a user downloads is about 60% smaller. Every test passes and `just relevance` is unchanged at nDCG@10 0.6403. The two largest levers left are both kept off: compiling dataseek for size (-525 to -558 KB) slows parsing, matching and merging by up to 75%, and `panic = "abort"` (-414 KB) would let one adapter's panic end the whole search.

## Caveats

- A live search over native-tls on macOS answered from 70 of 71 sources. The Linux binaries were tried in a `debian:trixie` container, arm64 natively and x86_64 under Rosetta: the release workflow's archive step, run as written, packed both targets; a fresh container checked each `.sha256` and unpacked the archive; both binaries printed their version and man page; without `ca-certificates` a search failed with the `Try:` line naming that package; with it, the v0.10.0 binary and this one, given the same query and `--timeout 0`, both answered from 70 of 71 sources, the same one (OpenNeuro, HTTP 502) failing for both.
- Debian's base image has no `xz`, so `tar xJf` on the archive fails there until `xz-utils` is installed. Desktop installs and most CI images carry it, and none of the documented installs needs the system's: Homebrew installs its own `xz` to unpack the formula's archive, `cargo binstall` decompresses in process, `uv` installs the wheel and `cargo install` builds from source.
- The Linux binary was cross-built with zig as the linker; the release job builds natively. Its relative changes are the result, its absolute size is not.
- Windows was not built. Its TLS stack and dependencies follow macOS, so its changes should too.
- The stub bounds overstate what a rewrite saves, because the rewrite's own code comes back: pulldown-cmark's stub saved 199 KB on macOS, the rewrite 182 KB.
