# CPU stages after the allocation and complexity pass, 2026-10-06

## Question

Which CPU stages of a search spend their time on avoidable copies or on work that grows faster than the input, and how much does removing it save without changing any output? Primary metric: Criterion's median wall time per call, witnessed by retired instruction counts because wall time on this machine is noisy ([baseline](2026-10-06-baseline.md)).

## Machine

```text
date                 2026-10-06 18:25 UTC
os                   macOS-27.0-arm64-arm-64bit-Mach-O
cpu                  Apple M4
performance_cores    4
efficiency_cores     6
memory_gib           16
power                Battery Power
battery_percent      31
low_power_mode       False
thermal_warning      False
rustc                1.99.0
host                 aarch64-apple-darwin
llvm                 23.1.1
load_1m              5.49
commit               40521d8
dirty                True
```

- warning: on battery: wall time follows the power manager
- warning: 1-minute load is 5.49: something else is running
- warning: efficiency cores present and macOS cannot pin threads: compare runs from one session

## Setup

- `just bench --save-baseline main` on the untouched tree (40521d8), then `just bench --baseline main` with the changes, both under `flock /tmp/heavy-build.lock`. The new `clean/ampersands` group was measured on an export of 40521d8 with the new bench file.
- Instruction counts: the same bench file under Criterion with a measurement that reads the benchmark thread's retired instructions (`thread_selfcounts`), in a scratch crate outside the repository, once against an export of 40521d8 and once against the changed tree. Counts repeat to within 0.01% between runs.
- Before and after were checked for identical output by a differential property test outside the repository: 20,000 random cases each for `clean`, catalog search, `merge` followed by `weigh`, and the Eurostat parser, with HTML, entities, bare ampersands, final sigma, dotted capital I and invisible characters in the inputs. A deliberately broken build failed it.

## Results

| stage | before | after | instructions before | after | change |
|---|---|---|---|---|---|
| `parse/sdmx/10000` | 8.34 ms | 6.80 ms | 154.4 M | 122.3 M | -20.8% |
| `parse/eurostat/16000` | 11.9 ms | 6.18 ms | 230.4 M | 118.6 M | -48.5% |
| `catalog/search/16000` | 11.7 ms | 3.15 ms | 227.7 M | 77.4 M | -66.0% |
| `catalog/load/16000` | 2.94 ms | 2.81 ms | 52.7 M | 45.4 M | -13.8% |
| `merge/6080` | 10.5 ms | 6.82 ms | 208.7 M | 127.7 M | -38.8% |
| `clean/200` | 1.12 us | 945 ns | 23.1 K | 20.1 K | -13.0% |
| `clean/20000` | 60.6 us | 53.2 us | 1.167 M | 1.131 M | -3.1% |
| `clean/ampersands/2000` | 12.8 us | 7.29 us | 300 K | 164 K | -45.4% |
| `clean/ampersands/20000` | 494 us | 72.4 us | 14.05 M | 1.61 M | -88.5% |

The smaller sizes of each group moved by the same share.

## Verdict

Every stage is cheaper, and no output changed. The one complexity bug was in `clean`: each `&` searched the whole rest of the text for the `;` that would end an entity, so prose with bare ampersands and no semicolon cost O(n m) for n bytes and m ampersands, 38x the time for 10x the input. Bounding the search to the longest entity name makes it linear. Everything else was allocation: catalog search tokenized every entry into a vector of owned words and now folds it into one reused buffer, where a term is a word prefix exactly when the term led by a space occurs; the top results come from introselect instead of a full stable sort; merge sorts small keys instead of moving whole hits; the SDMX parser walks each element's attributes once; cached catalogs are checked as UTF-8 once instead of per string.

## Caveats

- Wall time comes from a machine on battery with other work running; the instruction counts are the claim, the milliseconds are a rough guide.
- The instruction counts are Apple Silicon counts and are not comparable with CodSpeed's Linux simulation, which judges the pull request (ADR 0012).
- The tree was dirty with the changes under test; the commits that follow 40521d8 on this branch identify the measured code.
