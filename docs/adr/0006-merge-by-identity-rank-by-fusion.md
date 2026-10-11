# ADR 0006: Merge by identity, rank by fusion and query coverage

- Status: superseded by [ADR 0016](0016-rank-by-fusion-and-weighted-coverage.md)
- Date: 2026-10-06

## Context

Results arrive as about 75 ranked lists with incomparable scores (Elasticsearch relevance, download counts, catalog order). The same dataset appears in several (a Zenodo record in Zenodo, DataCite, OpenAIRE and Google), DataCite lists every version of a record, and many sources match loosely (any word, stemmed, or in fields dataseek never sees).

## Decision

`src/dedup.rs` does three things in order.

1. **Identity.** Two records are one dataset when they share a key: a DOI (normalized, also when it hides in a resolver or `/doi/` URL), a concept DOI carried as an alias (folds DataCite and Zenodo versions), a normalized landing URL (no scheme, `www.`, fragment, trailing slash or `utm_` parameters), or a normalized title with the same publisher. A title key only joins records from different sources. Merging is transitive. The first source's fields win; later sources fill gaps.
2. **Fusion.** Reciprocal rank fusion (Cormack, Clarke and Buettcher, SIGIR 2009) with k = 60: a hit scores the sum over its sources of `1 / (61 + rank)`, counting each source's best rank once. Agreement between sources beats any single first place.
3. **Coverage.** The fused score is scaled by `0.25 + 0.75 * coverage`, the share of meaningful query words in the title or description; a hit that shows a description yet mentions no query word is dropped.

## Consequences

- Scores need no calibration across sources, and a new source needs none either.
- A landing URL must carry its identity in the path or query, never only in the fragment: Data Commons links of the form `statvar#sv=ID` normalized to one URL and merged 45 variables into 5 until they were changed to `/browser/ID`.
- Reverting coverage weighting brings back off-topic hits that two loose sources agree on (NADA mirrors returned a household survey for "single cell lung").

## Evidence

- Bench 2026-10-06: 2,463 results from 76 sources over six queries; 2,355 merged hits came from exactly one source, so cross-source overlap is about 4%. The largest pairwise overlaps were data.gov and Socrata (4), DBnomics and Eurostat (4), and Kaggle and Google (3 per six queries).
- DataCite: 60 results contributed 39 hits, the rest being versions and duplicates folded by concept DOI and URL.
