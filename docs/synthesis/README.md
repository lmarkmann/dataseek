# Synthesis

What reads across several dataset sources at once: how registries such as Hugging Face, Kaggle or Zenodo differ in search semantics, metadata, licensing fields and rate limits, and what that means for one query surface over all of them. A file lands here when the same comparison is wanted by more than one source adapter.

| the fact | its home |
|---|---|
| a comparison across sources | **here** |
| one source's API shape, used by one adapter | the adapter's module docstring in `src/sources/` |
| a decision drawn from a comparison | [`../adr/`](../adr/), citing the file here |
| what shipped | [`../CHANGELOG.md`](../CHANGELOG.md) |

| file | covers |
|---|---|
| [search-methods.md](search-methods.md) | endpoint, query syntax, matching, ranking, paging and rate limits of every search interface, plus the quirks that shaped the adapters |
| [bench-2026-10-06.md](bench-2026-10-06.md) | latency, answer rate, results and overlap per source from `dataseek bench` |
