# ADR 0007: A 30 MB cache, a search deadline, and sources that fail alone

- Status: amended by [ADR 0018](0018-a-search-never-waits-for-a-catalog-download.md) (catalog downloads) and [ADR 0019](0019-stop-waiting-past-a-quorum.md) (stragglers)
- Date: 2026-10-06

## Context

A search that waits for 75 hosts is only as fast as the slowest, and some hosts fail or rate-limit. Catalog sources download lists of up to tens of thousands of entries (World Bank indicators: 29,533). The cache must not bloat.

## Decision

- **Cache** (`src/cache.rs`): one JSON file per entry under the XDG cache directory, written atomically. Query results live 6 hours, catalogs 7 days. The directory is trimmed to 30 MB and 2,000 files, oldest first, once per search (the entry cap follows yoink's bounded search cache). Kaggle results are never written, per Kaggle's terms.
- **Deadline**: `search` prints what has arrived after `--timeout` seconds (default 20; 0 waits for all). Sources still working are reported as still running; their threads are detached. `dataseek cache warm` downloads every catalog with no deadline.
- **Failure isolation** (`src/search.rs`): each source runs on its own thread. A transient connect failure is retried once after 300 ms; a short `Retry-After` is honored once. An outage (unreachable, timeout, challenge page, HTTP 5xx, or a rate limit that outlasts the one honored retry) marks the source for 10 minutes and later searches skip it unless it is named with `--source`. A failed fetch serves the expired cache entry when one exists. A required key that is missing skips the source instead of failing it.
- **Exit codes**: 0 when any source answered (even with no results), 1 when every attempted source failed or none could run.

## Consequences

- A cold first search can miss the slowest catalogs (PhysioNet took 35 s, OpenNeuro 25 s); the stderr summary names them and points at `cache warm`.
- A stale answer is labeled in `-v` and `--json` output, never silently passed off as fresh.
- Raising the budget or TTLs is one constant each.

## Evidence

- Cold search 2026-10-06 without a deadline: 35.5 s, bounded by PhysioNet. With the 20 s deadline: 20.3 s. Warm: under 1 s from cache, about 8 s fresh.
- `cache warm` fetched all 26 catalogs in about 30 s; the cache then held well under the budget.
- Concurrent lookups produced spurious DNS and `WouldBlock` errors for gbif and dandi until the single connect retry was added.
