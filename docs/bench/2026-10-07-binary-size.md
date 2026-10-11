# Release binary size, 2026-10-07

## Question

Where do the release binary's bytes go, what does each size lever save, and which levers are worth their cost (issue #20)? Primary metric: the file size of the release `dataseek` binary per target, which is what a download costs. Speed must not regress: Criterion on the CPU stages of a search, the startup bench, and CodSpeed on the pull request.

## Machine

```text
date                 2026-10-07 01:20-01:40 UTC
os                   macOS 27.0 arm64
cpu                  Apple M4
performance_cores    4
efficiency_cores     6
memory_gib           16
power                battery, 66% and discharging at the end
rustc                1.99.0
host                 aarch64-apple-darwin
llvm                 23.1.1
load_1m              4 to 6
cross builds         cargo-zigbuild with zig 0.17.0
size tool            cargo-bsize 0.0.2
baseline commit      4ec54bb (v0.6.0)
```

## Setup

- Every build ran under `flock /tmp/heavy-build.lock`, so nothing else compiled while a size or a time was taken.
- macOS targets are built with `cargo build --release --locked --target <t>`. Linux and Windows targets are built with `cargo zigbuild --release --locked --target <t>`. `x86_64-pc-windows-msvc` was not built: it needs cargo-xwin or a Windows machine, and neither is available here. Its size should follow `x86_64-pc-windows-gnu`, which links the same Rust code.
- `cargo bsize --bin dataseek` ranked crates, functions and generics. `cargo bsize --what-if --levers=all` rebuilt once per lever, each lever alone on top of the v0.6.0 profile.
- Each profile variant was built from the same tree with `--config profile.release...` overrides, so variants differ only in the profile.
- Speed: `just bench --save-baseline main` on v0.6.0, then `just bench --baseline main` per candidate profile; `just bench-startup` before and after.

## Results

File size of the release binary. "Profile" is the release profile change alone, "final" adds dropping `--jq`.

| target | v0.6.0 | profile | final |
|---|---|---|---|
| `aarch64-apple-darwin` | 4.94 MB | 3.79 MB (-23.2%) | 2.42 MB (-51.1%) |
| `x86_64-apple-darwin` | 5.26 MB | 4.23 MB (-19.7%) | 2.65 MB (-49.6%) |
| `x86_64-unknown-linux-gnu` | 6.90 MB | 5.57 MB (-19.3%) | 3.87 MB (-43.9%) |
| `aarch64-unknown-linux-gnu` | 6.05 MB | 4.84 MB (-20.0%) | 3.36 MB (-44.5%) |
| `x86_64-unknown-linux-musl` | 6.64 MB | 5.37 MB (-19.1%) | 3.73 MB (-43.9%) |
| `aarch64-unknown-linux-musl` | 5.84 MB | 4.65 MB (-20.3%) | 3.22 MB (-44.8%) |
| `x86_64-pc-windows-gnu` | 5.61 MB | 4.57 MB (-18.5%) | 2.75 MB (-51.0%) |

### Levers

Measured on `aarch64-apple-darwin`. The `--what-if` column is cargo-bsize's shipped size (code, data and unwind tables, 4.5 MiB at v0.6.0), each lever alone. The file column is the stripped release binary, 4,938,704 bytes at v0.6.0.

| lever | `--what-if` | file | cost | adopted |
|---|---|---|---|---|
| `lto = "fat"` | -272 KiB | 4.55 MB (-7.9%) | release build of the crate 10 s -> 42 s with dependencies cached | yes |
| `opt-level = "z"` for dependencies, with fat LTO | | 3.79 MB (-23.2%) | none measured (below) | yes |
| `opt-level = "z"` for dependencies, thin LTO | | 4.70 MB (-4.7%) | | no, fat LTO is what makes it pay |
| `opt-level = "z"` everywhere | -1.3 MiB | 4.39 MB thin, 2.97 MB fat | every CPU stage 43% to 287% slower | no |
| `opt-level = "s"` everywhere | | 5.06 MB thin (+2.4%), 3.74 MB fat | | no, larger than `z` and slower than 3 |
| `panic = "abort"` | -864 KiB | | a panicking adapter takes down the whole search | no |
| `codegen-units = 1` | 0 | | | already set |
| `share-generics=yes` | +240 KiB | | grows | no |
| `-C no-vectorize-loops` | -32 KiB | | slower hot loops for 0.7% | no |
| `-C force-unwind-tables=no` | -864 KiB | | only valid with `panic = "abort"` | no |
| `fmt-debug=none` | -224 KiB | | nightly only | no |
| `location-detail=none` | -96 KiB | | nightly only | no |
| `build-std` | +16 KiB | | nightly only, and grows | no |
| `optimize_for_size` (std feature) | -288 KiB | | nightly only, through build-std | no |
| `panic = "immediate-abort"` | -1.2 MiB | | nightly only, and the `panic = "abort"` cost | no |
| `min-size` (all of the above) | -2.8 MiB | | nightly only | no |
| `virtual-function-elimination` | | | the build failed on stable | no |

Every dependency at `opt-level = "z"` turned out no slower than keeping the speed-relevant ones (quick-xml, serde_json, pulldown-cmark, memchr, the number formatters, gzip) at 3, and 82 KB smaller. Fat LTO inlines their hot functions into `dataseek`'s own code, which stays at 3, so no exception list is kept.

### Speed

Criterion, `just bench --baseline main`, median change against v0.6.0:

| stage | fat LTO, dependencies at `z` (adopted) | `dataseek` at `z` as well |
|---|---|---|
| `parse/sdmx` 100 / 1k / 10k | -1.7% / -3.1% / -2.6% | +82% / +81% / +77% |
| `parse/eurostat` 1k / 4k / 16k | -3.9% / -4.5% / -3.8% | +108% / +105% / +106% |
| `catalog/search` 1k / 4k / 16k | -6.8% / -8.5% / -7.1% | +225% / +215% / +217% |
| `catalog/load` 1k / 4k / 16k | -7.4% / -2.5% / -5.7% | +43% / +50% / +46% |
| `merge` 380 / 1520 / 6080 | -5.1% / -8.2% / -8.0% | +106% / +98% / +91% |
| `clean` 200 / 2k / 20k | -3.5% / -4.4% / -3.6% | +281% / +286% / +287% |

`just bench-startup`, mean ms, v0.6.0 -> adopted profile: `--version` 3.0 -> 2.6, `--help` 3.1 -> 2.7, bare 2.8 -> 2.5, `completion fish` 3.4 -> 3.2, `sources` 3.7 -> 2.7. All within noise or faster, all far inside their budgets.

### Where the bytes went

`just bloat --baseline` against v0.6.0, by crate: `core` -552 KiB, `clap_builder` -97 KiB, `alloc` -65 KiB, `pulldown_cmark` -38 KiB, `ureq_proto` -37 KiB, `ureq` -37 KiB from the profile; `jaq_core` -164 KiB, `jiff` -113 KiB, `jaq_json` -97 KiB, `aho_corasick` -81 KiB, `jaq_std` -80 KiB, `num_bigint` -57 KiB, `jiff_core` -41 KiB, `libm` -39 KiB, `regex_bites` -34 KiB removed with `--jq`.

### Dependencies

- `toml`, `signal-hook` and `indicatif` lost the default features dataseek never uses (`display`, `channel` and `iterator`, `wasmbind`). Thin LTO had already dropped that code, so the binary did not change; the lockfile lost 13 crates.
- `ureq` on macOS and Windows still bundles the Mozilla root store (`webpki-root-certs`), which the platform verifier never reads. ureq 3.4.2 compiles its native-tls connector only under the `native-tls` feature, which always pulls the roots in; `native-tls-no-default` builds, then panics on the first HTTPS request. See the watch list.
- The duplicate `base64` (0.22 through jaq-std, 0.23 through ureq) left with `--jq`.

### --jq

| build, on the adopted profile | `aarch64-apple-darwin` |
|---|---|
| with jaq, every built-in | 3,792,624 bytes |
| jaq-core and jaq-json only, no jaq-std built-ins | 3,161,808 bytes |
| without `--jq` | 2,415,024 bytes |

`--jq` cost 1.38 MB, 36% of the binary after the profile change. jaq-std exposes its native built-ins only with every one of its features on, so a cheaper `--jq` would be missing `test`, `split`, `@csv`, `floor`, dates and more, failing on expressions a jq user expects to work. A default-on cargo feature would save nothing in the binaries people download. `--jq` was dropped; `--json | jq` does the same.

## Verdict

The release profile now uses fat LTO and builds every dependency at `opt-level = "z"`, while dataseek's own code stays at 3. The binary is 18% to 23% smaller per target, and no measured stage got slower. With `--jq` dropped, the binary is 44% to 51% smaller than v0.6.0. `panic = "abort"` saved 864 KiB on the v0.6.0 profile, but it would turn one adapter's panic into a failed search, so it stays off.

## Caveats

- Local wall time on this machine is direction only; an A/A run once reported identical code 52% faster ([baseline](2026-10-06-baseline.md)). The adopted profile's changes are small and all in the faster direction. The `dataseek` at `z` regression is far beyond the noise. CodSpeed's CPU simulation on the pull request is the verdict.
- No benchmark covers gzip decompression or HTTP parsing, which now run at `opt-level = "z"`. Both are dominated by network time, which runs from tenths of a second to seconds per source.
- The Linux and Windows binaries were cross-built with zig as the linker, and a release built on each platform may link differently. Their relative changes are the result; their absolute sizes are not.
