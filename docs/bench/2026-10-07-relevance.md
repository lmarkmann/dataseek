# Relevance of the merged ranking, 2026-10-07

## Question

How good is the top 10 of a default search, and which change to merging and ranking makes it better on queries it was not tuned on? Primary metric: nDCG@10 over graded queries; known-item reciprocal rank must not fall. The method and the commands are in [`../reference/development.md`](../reference/development.md#relevance).

## Setup

- Snapshot recorded 2026-10-07 with dataseek 0.6.0 (`just relevance record`): 48 queries against the 73 default sources that may be stored (Kaggle is `persist: false` and never recorded), 10 results asked of each, no key, an empty cache. 68 sources answered every query and Hugging Face 46 of 48: it answered HTTP 400 on `iris` and `github-issues`. FRED, Roboflow and Data Commons were skipped for want of a key, and WHO failed on every query with a shape error ("not JSON"), which is an adapter defect outside this measurement. 11,657 records, 5.99 MB as committed and 1.58 MB gzipped.
- 882 judgments, one per dataset and query, over the pooled top 10 of every variant below and of the shipped ranking: 378 not relevant, 229 partly, 275 relevant. Labels were assigned blind to source and rank from the worksheets `just relevance pool` writes, with the rubric at the head of `judgments.tsv`: four grading passes under that one rubric, each over whole queries, then the rounds that followed as new variants surfaced new results.
- Known items are scored against `targets` in `queries.toml`, each with the page or registry entry that confirmed it. MIMIC-IV was retrieved by no source (only its 100-patient demo and separate modules came back), so its reciprocal rank is 0 for every ranking.
- Intervals are percentile bootstrap 95% over queries, 10,000 resamples; a change between rankings is the paired bootstrap of per-query differences. The bounds move by at most 0.0044 under two other seeds.

## Results

The shipped ranking before and after the adopted change, on the final labels:

| metric | half | before | after |
|---|---|---|---|
| nDCG@10 (32 graded) | all | 0.626 [0.538, 0.710] | 0.650 [0.575, 0.725] |
| | tune | 0.642 [0.503, 0.780] | 0.663 [0.536, 0.789] |
| | held | 0.609 [0.505, 0.703] | 0.637 [0.561, 0.716] |
| P@10 (32 graded) | all | 0.697 [0.594, 0.794] | 0.722 [0.628, 0.809] |
| | tune | 0.663 [0.512, 0.806] | 0.688 [0.538, 0.825] |
| | held | 0.731 [0.587, 0.850] | 0.756 [0.637, 0.856] |
| known-item MRR (16) | all | 0.514 [0.304, 0.730] | 0.517 [0.308, 0.733] |
| | tune | 0.474 [0.199, 0.781] | 0.480 [0.210, 0.781] |
| | held | 0.554 [0.226, 0.887] | 0.554 [0.226, 0.887] |
| known items in the top 10 | | 13 of 16 | 13 of 16 |

Every variant, each alone on top of the ranking shipped before (nDCG@10 by half, the paired held-out change, MRR by half):

| variant | tune | held | held change [95% CI] | MRR tune | MRR held | adopted |
|---|---|---|---|---|---|---|
| shipped: RRF k = 60, term coverage | 0.642 | 0.609 | | 0.474 | 0.554 | |
| RRF k = 1 | 0.575 | 0.525 | -0.084 [-0.143, -0.026] | 0.505 | 0.580 | no |
| RRF k = 10 | 0.657 | 0.600 | -0.009 [-0.060, +0.032] | 0.480 | 0.559 | no |
| RRF k = 20 | 0.653 | 0.584 | -0.025 [-0.070, +0.003] | 0.480 | 0.556 | no |
| RRF k = 40 | 0.642 | 0.615 | +0.006 [+0.000, +0.017] | 0.474 | 0.554 | no |
| RRF k = 100 | 0.642 | 0.609 | +0.000 | 0.470 | 0.554 | no |
| RRF k = 200 | 0.641 | 0.609 | +0.000 | 0.470 | 0.554 | no |
| top 3 per source | 0.606 | 0.608 | -0.001 [-0.078, +0.076] | 0.526 | 0.582 | no |
| top 5 per source | 0.614 | 0.580 | -0.029 [-0.094, +0.032] | 0.505 | 0.570 | no |
| CombMNZ | 0.641 | 0.597 | -0.012 [-0.028, -0.000] | 0.466 | 0.552 | no |
| Borda, list-length normalized | 0.648 | 0.574 | -0.035 [-0.100, +0.028] | 0.492 | 0.569 | no |
| round robin | 0.535 | 0.507 | -0.102 [-0.166, -0.039] | 0.507 | 0.596 | no |
| source priors from tuning labels | 0.702 | 0.600 | -0.010 [-0.075, +0.056] | 0.385 | 0.553 | no |
| coverage floor 0 | 0.637 | 0.584 | -0.025 [-0.064, -0.000] | 0.480 | 0.567 | no |
| coverage floor 0.1 | 0.641 | 0.592 | -0.017 [-0.051, +0.000] | 0.480 | 0.556 | no |
| coverage floor 0.5 | 0.641 | 0.593 | -0.016 [-0.033, -0.002] | 0.469 | 0.552 | no |
| coverage squared | 0.641 | 0.604 | -0.005 [-0.026, +0.011] | 0.480 | 0.567 | no |
| title-weighted coverage | 0.638 | 0.619 | +0.010 [-0.053, +0.071] | 0.495 | 0.554 | no |
| **IDF-weighted coverage** | **0.663** | **0.637** | **+0.028 [+0.004, +0.066]** | 0.480 | 0.554 | **yes** |
| no rescoring (plain RRF) | 0.499 | 0.480 | -0.129 [-0.210, -0.058] | 0.450 | 0.539 | no |
| keep described hits that match no term | 0.634 | 0.609 | +0.000 | 0.474 | 0.554 | no |
| ignore function words | 0.673 | 0.605 | -0.004 [-0.054, +0.032] | 0.474 | 0.554 | no |
| whole query in the title x1.5 | 0.636 | 0.611 | +0.002 [-0.051, +0.045] | 0.422 | 0.578 | no |
| whole query in the title x2 | 0.635 | 0.621 | +0.012 [-0.045, +0.065] | 0.453 | 0.632 | no |
| thin records x0.5 / x0.75 / dropped when unmatched | 0.642 | 0.609 | +0.000 | 0.474 | 0.554 | no |
| greedy stack on tune: priors + function words + coverage squared + floor 0.5 | 0.747 | 0.608 | -0.001 [-0.080, +0.082] | 0.385 | 0.552 | no |

The decision was taken on the labels that existed when it was made, 806 once each dataset is graded once per query: +0.047 [+0.004, +0.121] held out. The 76 labels added since, for results that later variants surfaced, shrank the estimate to the +0.028 above without changing the decision. The greedy stack rows carry up to five unjudged results, counted as not relevant.

Re-run on top of the adopted ranking, no family's tuning-half winner improves the held-out half (the best, coverage squared, +0.007 [-0.018, +0.030]); `just relevance variants` prints that table.

Pollution of the top 10 over the graded queries (slots a source fills, of which not relevant):

| source | before | after |
|---|---|---|
| openaire | 100, 18 | 102, 19 |
| zenodo | 62, 17 | 63, 15 |
| europa | 29, 14 | 26, 11 |
| datacite | 72, 12 | 73, 13 |
| data-gov-uk | 20, 9 | 16, 7 |
| b2find | 20, 7 | 20, 7 |
| every slot | 320, 97 (30.3%) | 320, 89 (27.8%) |

## Verdict

Only one change beat the noise: weighting each query term by its inverse document frequency among the merged hits before measuring coverage. The fusion side is flat. Every RRF constant from 40 up scores within 0.006 of 60, because with 4% overlap the constant only reorders the few hits that two sources share; smaller constants, Borda and round robin interleave sources more aggressively and lose. Source priors are the clearest case of tuning to the benchmark: +0.060 on the tuning half they were fitted on, -0.010 held out. The rescoring step carries the ranking (dropping it costs 0.129 against the old ranking and 0.157 against the adopted one), and IDF is the one refinement of it that holds up. Its gain is entirely on natural-language queries: all 16 topical queries keep their nDCG@10, while the 16 natural-language ones gain 0.049 on average (10 better, 3 worse, the worst by 0.018, the best "labeled images of skin lesions for melanoma detection" by 0.276). A sentence carries words that most of its hits share, and under plain coverage each counted as much as the one word that names the data.

## Caveats

- The labels come from four grading passes under one rubric, not from an independent panel; they are a proposal. `just relevance variants` lists the labels whose flip would change the decision.
- Hugging Face failed on `iris`, and the Hugging Face copy of the dataset is one of that query's targets, so its reciprocal rank was measured without one of the sources that could have found it.
- DBnomics returns some Eurostat tables twice, under links that differ only in the case of the table code; dataseek does not merge them, so each copy is a separate result with its own label.
- With 16 graded queries per half, the held-out intervals are wide; +0.028 is the estimate, not a promise.
- Rare tokens that are not topical get large weights too: "monthly inflation by country since 2000" now ranks an income table "in 2000$" first.
- The snapshot has no Kaggle and none of the three keyed sources, so it measures ranking over the lists a keyless search sees.
- Catalog sources require every query term to match (`src/catalog.rs`), so long natural-language queries retrieve almost nothing from them; the ECB catalog returned nothing for "ECB euro foreign exchange reference rates". That is retrieval, which a frozen snapshot cannot measure.
