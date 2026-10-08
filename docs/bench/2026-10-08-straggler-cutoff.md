# What it costs to stop waiting for the slowest sources, 2026-10-08

## Question

A default search lasts as long as its slowest source. How much faster is a search that stops waiting once most sources have answered, and how much of the merged top 10 does it lose? Primary measures: wall time, the share of the full top 10 still shown, and nDCG@10 and known-item reciprocal rank against the relevance judgments ([`2026-10-07-relevance.md`](2026-10-07-relevance.md)).

## Setup

- `tests/fixtures/relevance/latency.jsonl`: the 48 relevance queries, each searched once with `search --json --timeout 0 --refresh` by dataseek 0.8.0 plus the cold-start branch, on an Apple M4 over a home connection in Germany, 2026-10-08, with every catalog cached and no key. Each line keeps every source's status and milliseconds. These are first asks: DataONE answers a repeated query in 0.46 s and a new one in about 2.5 s, so a second round would understate the tail.
- `just relevance cutoff tests/fixtures/relevance/latency.jsonl` replays each rule against the recorded per-source lists: a source that would have answered after the rule stopped waiting gets an empty list, and merging and ranking run as shipped. Catalog sources count as answering at once, as they do from the cache (ADR 0018).
- Deltas are against waiting for every source, with paired bootstrap 95% intervals over queries (10,000 resamples, fixed seed).

## Results

| rule | p50 | p90 | max | sources left out | full top 10 kept | nDCG@10 tune | nDCG@10 held | known-item MRR |
|---|---|---|---|---|---|---|---|---|
| wait for all | 3.55 s | 9.57 s | 11.99 s | 0 | 100% | | | |
| deadline 1 s | 1.00 s | 1.00 s | 1.00 s | 4.12 | 77.1% | -0.065 [-0.105, -0.030] | -0.018 [-0.081, +0.037] | +0.017 [+0.003, +0.032] |
| deadline 2 s | 2.00 s | 2.00 s | 2.00 s | 1.04 | 91.5% | -0.033 [-0.056, -0.013] | +0.009 [-0.009, +0.031] | +0.006 [+0.000, +0.017] |
| 80% + 1 s | 1.60 s | 1.66 s | 1.76 s | 1.44 | 89.8% | -0.033 [-0.056, -0.013] | -0.002 [-0.028, +0.025] | +0.006 [+0.000, +0.017] |
| 90% + 0 s | 0.86 s | 0.98 s | 1.05 s | 5.65 | 72.3% | -0.087 [-0.131, -0.043] | -0.027 [-0.089, +0.028] | +0.058 [+0.003, +0.156] |
| 90% + 0.5 s | 1.36 s | 1.49 s | 1.55 s | 1.94 | 85.8% | -0.041 [-0.066, -0.019] | -0.019 [-0.058, +0.019] | +0.008 [+0.000, +0.020] |
| **90% + 1 s** | **1.83 s** | **1.97 s** | **2.05 s** | **1.08** | **91.0%** | **-0.033 [-0.056, -0.013]** | **+0.002 [-0.024, +0.028]** | **+0.006 [+0.000, +0.017]** |
| 90% + 2 s | 2.75 s | 2.95 s | 3.02 s | 0.79 | 94.4% | -0.036 [-0.067, -0.011] | +0.009 [-0.010, +0.032] | +0.006 [+0.000, +0.017] |
| 95% + 0.5 s | 1.64 s | 2.00 s | 2.99 s | 1.21 | 91.0% | -0.032 [-0.055, -0.013] | +0.007 [-0.011, +0.030] | +0.006 [+0.000, +0.017] |
| 95% + 2 s | 2.99 s | 3.48 s | 4.49 s | 0.75 | 94.6% | -0.035 [-0.066, -0.010] | +0.009 [-0.010, +0.032] | +0.001 [+0.000, +0.002] |

The shipped nDCG@10 is 0.663 on the tuning half and 0.637 on the held-out half, so the tuning-half loss of 0.033 is about 5% of it.

Under 90% + 1 s, the sources left out were DataONE in 31 of 48 searches (median 3.7 s when late), Zenodo in 11 (5.6 s), DBnomics and DataCite in 4 each, CESSDA, recherche.data.gouv and BioStudies in 2, and Hugging Face, DANDI and OmicsDI in 1.

## Reading

- Every rule that cuts anything costs about the same 0.03 of tuning-half nDCG, from deadline 2 s to 95% + 2 s: the loss is the one source that is almost always late, DataONE, and Zenodo when it is slow, not the rule. Waiting longer buys little until the wait covers DataONE's first-ask latency, which is waiting for all.
- The held-out half shows no loss under any rule from 2 s up, and known-item MRR rises slightly: the late sources put a little less noise above the targets than they add relevant results.
- 90% + 1 s gives the tightest worst case at the median loss: a search ends within about 2 s instead of 3.6 s at the median and 9.6 s at the 90th percentile.
- DataONE's time is its server's: the same Solr request takes 2.5 s for a new query and 0.46 s repeated, with or without the abstract field and the filter query. Zenodo answered new queries in 0.3 to 1.1 s on a later check, so its slow answers here were load on its side.
