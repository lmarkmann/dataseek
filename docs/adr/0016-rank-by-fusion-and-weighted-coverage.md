# ADR 0016: Merge by identity, rank by fusion and rarity-weighted coverage

- Status: accepted; supersedes [ADR 0006](0006-merge-by-identity-rank-by-fusion.md)
- Date: 2026-10-07

## Context

Results arrive as about 75 ranked lists with incomparable scores, and only about 4% of merged hits are found by more than one source (ADR 0006's bench). Fusion therefore mostly interleaves lists that rarely agree, and the order of the top 10 rests on the coverage step that rescales each hit by how much of the query it mentions. ADR 0006 counted every query word the same, so in a sentence such as "monthly inflation by country since 2000" a hit carrying "country" and "by" outscored one carrying only "inflation". The relevance benchmark (`just relevance`, [`../reference/development.md`](../reference/development.md#relevance)) now measures the merged top 10 offline against graded judgments, which makes ranking changes testable instead of argued.

## Decision

`src/dedup.rs` does three things in order.

1. **Identity.** Unchanged from ADR 0006: two records are one dataset when they share a DOI (also a concept DOI or one hidden in a resolver link), a normalized landing URL, or a normalized title with the same publisher; a title key only joins different sources; merging is transitive; the first source's fields win.
2. **Fusion.** Unchanged: reciprocal rank fusion (Cormack, Clarke and Buettcher, SIGIR 2009, doi:10.1145/1571941.1572114) with k = 60 over each source's best rank.
3. **Coverage.** The fused score is scaled by `0.25 + 0.75 * coverage`, where coverage is the summed weight of the query terms found in the title or description over the weight of all of them. A term weighs its inverse document frequency among the hits being ranked, in the BM25 form `ln(1 + (n - df + 0.5) / (df + 0.5))` (Robertson and Zaragoza, Foundations and Trends in Information Retrieval 2009, doi:10.1561/1500000019), so a word nearly every hit carries counts for little. A described hit that mentions no term is dropped, as before.

The weights come from the merged hits of the query itself, not from a corpus: with no shared index across sources, the candidate set is the only collection the query has, which is the broker-side scoring of returned summaries the federated search literature describes for uncooperative sources (Shokouhi and Si, Foundations and Trends in Information Retrieval 2011, doi:10.1561/1500000010).

## Consequences

- Scores still need no calibration across sources, and a new source needs none.
- Short keyword queries rank as before; the change moves sentence-like queries, which agents send.
- A rare token that is not topical, a year or a code, also weighs a lot: "since 2000" lifts a table "in 2000$".
- Only the first 64 query terms are weighed. Terms past the 64th are ignored, so a hit matching only those counts as matching nothing.
- Against the adopted ranking, as `just relevance variants` measures it, reverting to plain coverage costs 0.028 nDCG@10 on the held-out queries and dropping coverage altogether costs 0.157.
- A change to this file is measured by `just relevance`, which CI runs; the gate fails when nDCG@10, P@10 or known-item MRR falls by more than a quarter of its bootstrap standard error, or when one query slips on its own (nDCG@10 down by more than 0.1, or a known item out of the top 10).

## Evidence

- Relevance benchmark 2026-10-07 ([`../bench/2026-10-07-relevance.md`](../bench/2026-10-07-relevance.md)): 48 queries, 11,657 recorded records, 882 judgments. Among 26 single changes (RRF constants, fusion depth, CombMNZ, Borda, round robin, source priors, coverage floors and shapes, title weighting, phrase bonus, function words, thin records) and a greedy stack, only IDF weighting improved the held-out half beyond the noise: nDCG@10 +0.028, paired 95% interval [+0.004, +0.066], with known-item MRR unchanged (0.554).
- Shipped ranking before and after, all 32 graded queries: nDCG@10 0.626 [0.538, 0.710] to 0.650 [0.575, 0.725]; P@10 0.697 to 0.722; non-relevant top-10 slots 97 of 320 to 89.
- Source priors fitted on the tuning half gained 0.060 there and lost 0.010 held out; RRF constants from 40 to 200 moved nDCG@10 by at most 0.006.
