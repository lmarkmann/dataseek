# What it costs to stop waiting for the slowest sources, 2026-10-08

## Question

A default search lasts as long as its slowest source. How much faster is a search that stops waiting once most sources have answered, and how much of the merged top 10 does it lose? Primary measures: wall time, the share of the full top 10 still shown, and nDCG@10 and known-item reciprocal rank against the relevance judgments ([`2026-10-07-relevance.md`](2026-10-07-relevance.md)).

## Setup

- `tests/fixtures/relevance/latency.jsonl`: the 48 relevance queries, each searched once with `search --json --timeout 0 --refresh` by dataseek 0.8.0 plus the cold-start branch, on an Apple M4 over a home connection in Germany, 2026-10-08, with every catalog cached and no key. Each line keeps every source's status and milliseconds. These are first asks: DataONE answers a repeated query in 0.46 s and a new one in about 2.5 s, so a second round would understate the tail.
- `just relevance cutoff tests/fixtures/relevance/latency.jsonl` replays each rule in `RULES` (`examples/relevance/cutoff.rs`) against the snapshot's per-source lists: a source that would have answered after the rule stopped waiting gets an empty list, and merging and ranking run as shipped. A quorum rule is counted as the search loop counts it, through its own `quorum`: answers out of the sources that have not failed. Catalog sources count as answering at once, as they do from the cache (ADR 0018).
- Deltas are against waiting for every source, with paired bootstrap 95% intervals over queries (10,000 resamples, fixed seed). The snapshot is the one `just relevance` scores, Hugging Face's lists included as recorded on 2026-10-08.

## Results

| rule | p50 | p90 | max | sources left out | full top 10 kept | nDCG@10 tune | nDCG@10 held | known-item MRR |
|---|---|---|---|---|---|---|---|---|
| wait for all | 3.55 s | 9.57 s | 11.99 s | 0 | 100% | | | |
| deadline 1 s | 1.00 s | 1.00 s | 1.00 s | 4.15 | 79.6% | -0.043 [-0.072, -0.016] | +0.006 [-0.054, +0.066] | +0.018 [+0.004, +0.034] |
| deadline 2 s | 2.00 s | 2.00 s | 2.00 s | 1.06 | 91.7% | -0.033 [-0.057, -0.012] | +0.005 [-0.009, +0.021] | +0.006 [+0.000, +0.017] |
| deadline 3 s | 3.00 s | 3.00 s | 3.00 s | 0.81 | 94.6% | -0.035 [-0.064, -0.011] | +0.006 [-0.010, +0.029] | +0.006 [+0.000, +0.017] |
| 80% + 0 s | 0.61 s | 0.68 s | 0.76 s | 10.90 | 56.9% | -0.160 [-0.288, -0.063] | -0.043 [-0.136, +0.045] | +0.070 [+0.010, +0.170] |
| 80% + 1 s | 1.60 s | 1.66 s | 1.76 s | 1.46 | 90.2% | -0.033 [-0.058, -0.012] | -0.001 [-0.017, +0.017] | +0.006 [+0.000, +0.017] |
| 90% + 0 s | 0.86 s | 0.98 s | 1.05 s | 5.71 | 73.5% | -0.086 [-0.149, -0.031] | -0.040 [-0.110, +0.023] | +0.059 [+0.004, +0.157] |
| 90% + 0.5 s | 1.36 s | 1.49 s | 1.55 s | 1.96 | 86.0% | -0.040 [-0.065, -0.018] | -0.026 [-0.064, +0.007] | +0.008 [+0.000, +0.020] |
| **90% + 1 s** | **1.83 s** | **1.97 s** | **2.05 s** | **1.10** | **91.2%** | **-0.033 [-0.057, -0.012]** | **+0.001 [-0.016, +0.018]** | **+0.006 [+0.000, +0.017]** |
| 90% + 2 s | 2.75 s | 2.95 s | 3.02 s | 0.81 | 94.6% | -0.035 [-0.064, -0.011] | +0.006 [-0.010, +0.029] | +0.006 [+0.000, +0.017] |
| 95% + 0.5 s | 1.64 s | 2.00 s | 2.99 s | 1.23 | 91.2% | -0.032 [-0.056, -0.012] | +0.004 [-0.009, +0.020] | +0.006 [+0.000, +0.017] |
| 95% + 1 s | 2.05 s | 2.48 s | 3.49 s | 0.96 | 92.3% | -0.036 [-0.064, -0.013] | +0.013 [-0.007, +0.037] | +0.006 [+0.000, +0.017] |
| 95% + 2 s | 2.99 s | 3.48 s | 4.49 s | 0.75 | 95.0% | -0.034 [-0.062, -0.010] | +0.006 [-0.010, +0.029] | +0.001 [+0.000, +0.002] |

The snapshot's nDCG@10 is 0.655 on the tuning half and 0.626 on the held-out half, so the tuning-half loss of 0.033 is about 5% of it.

Under 90% + 1 s, the answering sources left out were DataONE in 31 of 48 searches (median 3.7 s when late), Zenodo in 11 (5.6 s), DBnomics and DataCite in 4 each, CESSDA, recherche.data.gouv and BioStudies in 2, and DANDI and OmicsDI in 1.

## Reading

- Every rule that waits at least about a second past most answers, or two seconds in all, costs about the same 0.033 of tuning-half nDCG, from deadline 2 s to 95% + 2 s: the loss is the one source that is almost always late, DataONE, and Zenodo when it is slow. Waiting longer buys little until the wait covers DataONE's first-ask latency, which is waiting for all. Shorter waits cost more: 0.043 at deadline 1 s, 0.086 at 90% + 0 s, 0.160 at 80% + 0 s.
- The held-out half shows no loss under any of those rules, and known-item MRR rises slightly: the late sources put a little less noise above the targets than they add relevant results.
- Among them, 80% + 1 s is fastest (1.60 s at the median, 1.76 s at worst) and leaves out 1.46 answering sources a search; 90% + 1 s takes 0.2 s longer and leaves out 1.10, keeping 91.2% of the full top 10 against 90.2%. A fixed 2 s deadline matches 90% + 1 s on every column here, but it is set for this connection: on a slower one it cuts every source that needs more than two seconds, while a quorum waits for most of them wherever they are. One connection's recording cannot show that difference, so it is the reason for the quorum rather than a measured result.
- DataONE's time is its server's: the same Solr request takes 2.5 s for a new query and 0.46 s repeated, with or without the abstract field and the filter query. Zenodo answered new queries in 0.3 to 1.1 s on a later check, so its slow answers here were load on its side.
