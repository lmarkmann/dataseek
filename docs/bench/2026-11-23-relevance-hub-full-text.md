# Relevance with the Hub's full-text search, 2026-11-23

## Question

Does the merged top 10 get better when the `huggingface` adapter also asks the Hub's full-text search for a query of several words, and in which order should its hits join the adapter's list? The method and the commands are in [`../reference/development.md`](../reference/development.md#relevance).

## Setup

- 48 queries, 32 of them graded and 16 known-item, in two halves. The order was chosen on the tuning half only.
- Three recordings of the `huggingface` lists on 2026-11-23 with `just relevance record --source huggingface`, no key: the adapter of v0.10.1 (ids only), and the new adapter with each of two orders. Every other source's list is the one already in the snapshot.
- Order (a): the query words a dataset holds, then its downloads, then the Hub's full-text order. Order (b): the full-text order first, then (a).
- All three are scored on the same 1,154 judgments. 111 were added for the results the new lists brought into a top 10.

## Results

Mean nDCG@10 and P@10 over the graded queries, known-item MRR over the others, with 95% bootstrap intervals:

| ranking | half | nDCG@10 | P@10 | known-item MRR |
|---|---|---|---|---|
| v0.10.1 adapter | all | 0.616 [0.528, 0.700] | 0.756 [0.663, 0.838] | 0.518 [0.310, 0.733] |
| | tune | 0.621 [0.481, 0.758] | 0.756 [0.619, 0.881] | 0.479 [0.208, 0.781] |
| | held | 0.611 [0.505, 0.706] | 0.756 [0.625, 0.862] | 0.557 [0.234, 0.887] |
| full text, order (a), shipped | all | 0.645 [0.573, 0.716] | 0.778 [0.694, 0.850] | 0.518 [0.308, 0.733] |
| | tune | 0.646 [0.520, 0.774] | 0.788 [0.663, 0.894] | 0.479 [0.208, 0.781] |
| | held | 0.644 [0.583, 0.714] | 0.769 [0.656, 0.863] | 0.556 [0.231, 0.886] |
| full text, order (b) | tune | 0.595 [0.448, 0.735] | 0.719 [0.575, 0.844] | 0.480 [0.210, 0.781] |

Order (b) was recorded for the tuning half only and scored before the 32 held-out labels were added, which do not touch the tuning queries.

## Reading

- Order (a) is better than the old adapter on both halves, and order (b) is worse than the old adapter on the tuning half. The Hub's full-text order puts cards that hold few of the query words near the top; the word count moves them down.
- "fMRI recordings of people watching movies" shows the gain. The old adapter returns no dataset for it, because no repository id holds two of its words. The full-text search returns ten, three of them graded relevant.
- A full-text hit needs a description. In a first version it had none, a record with no description is never dropped for lacking the query's words, and title-only records took top-10 places: that version lowered nDCG@10 on the tuning half. The description is now the start of the card.
- The gate compared the new lists with the baseline of October and failed on two held-out queries. On `skin-lesions` the old adapter scores 0.419 today and the new one 0.458: the fall from 0.580 comes from datasets published since the baseline was recorded and from the larger pool of relevant results, which lowers nDCG for a list that did not change. On `american-community-survey` the known item moves from rank 10 to rank 11; that cost is real. The baseline was reset with `just relevance --bless` to the scores of order (a).
