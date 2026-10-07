# Architecture decision records

A decision that constrains future work lives here, once, and is cited from wherever it is load-bearing. Config files and workflow headers carry a one-line pointer at most.

## What belongs here, and what does not

| the fact | its home |
|---|---|
| a decision that would be expensive to reverse or tempting to re-litigate | **here**, one numbered file each |
| something considered and left out, with the date | [`rejected.md`](rejected.md) |
| something parked as *not yet*, with the trigger that revisits it | [`watchlist.md`](watchlist.md) |
| how to run, configure or extend something | [`../reference/`](../reference/) |
| what reads across several dataset sources | [`../synthesis/`](../synthesis/) |
| a speed measurement of the code, with its machine | [`../bench/`](../bench/) |
| what shipped | [`../CHANGELOG.md`](../CHANGELOG.md) |
| an invariant a caller must not break | the source comment, in one line |

## Format

`NNNN-kebab-title.md` with four sections: **Context** (what forced the decision), **Decision** (present tense), **Consequences** (what it costs, and what breaks if it is reverted), **Evidence** (the measurement or incident that settled it). Under about fifty lines, except where the decision is a list: ADR 0004 carries the full source registry. Superseding an ADR means writing a new one and marking both, never editing the old one into agreement.

## The record

| # | decision | status |
|---|---|---|
| [0001](0001-release-plz-owns-the-version.md) | release-plz owns the version, the tag and the changelog | accepted |
| [0002](0002-one-linux-ci-job.md) | CI is one Linux job that cross-checks Windows and macOS | accepted |
| [0003](0003-the-cli-surface-sets-the-version.md) | The CLI surface sets the version | accepted |
| [0004](0004-every-source-is-searched.md) | Every source is searched, none is tiered; the full source list | accepted |
| [0005](0005-one-adapter-per-shared-protocol.md) | One adapter per shared protocol (CKAN, Dataverse, NADA, Socrata, STAC, SDMX, ...) | accepted |
| [0006](0006-merge-by-identity-rank-by-fusion.md) | Merge by identity, rank by fusion and query coverage | superseded by 0014 |
| [0007](0007-cache-budget-and-failure-handling.md) | A 30 MB cache, a search deadline, and sources that fail alone | accepted |
| [0008](0008-google-dataset-search-from-page-data.md) | Google Dataset Search is read from its results page data | superseded by 0013 |
| [0009](0009-keys-and-contact-address.md) | Keys come from the environment or files, never flags | accepted |
| [0010](0010-dsk-is-dataseek.md) | `dsk` is the same program under a short name | accepted |
| [0011](0011-tls-stack-per-platform.md) | The TLS stack is chosen per platform | accepted |
| [0012](0012-benchmarks-criterion-local-codspeed-ci.md) | Benchmarks run as Criterion locally and as CodSpeed's CPU simulation in CI | accepted |
| [0013](0013-opt-in-sources.md) | Opt-in sources are asked only when named | accepted |
| [0014](0014-rank-by-fusion-and-weighted-coverage.md) | Merge by identity, rank by fusion and rarity-weighted coverage | accepted |
