# ADR 0018: A search never waits for a catalog download it can do without

- Status: accepted; amends [ADR 0007](0007-cache-budget-and-failure-handling.md)
- Date: 2026-10-08

## Context

ADR 0007 downloads a missing or expired catalog inside the search and gives up on it at the deadline. Two catalogs take longer than the 20 second default to download: OpenNeuro pages its list 100 datasets at a time behind a cursor, and PhysioNet builds its one response for about 30 seconds. A thread past the deadline dies with the process before it stores anything, so these catalogs never reached the cache. Every search then waited the full 20 seconds and warned about them, on the first run, again a week later when the catalogs expired, and after every upgrade, since a catalog written by another release counts as expired.

Fetching the work in parallel does not remove the wait. The OpenNeuro cursor is base64 of an offset, so every page can be asked for at once, but the server is the limit: 20 pages took 42.3 s one at a time, 11.5 s four at a time (with one HTTP 502), 18.2 s eight at a time and 12.0 s twenty at a time. PhysioNet is one request. An async runtime would add what ADR 0015 measured against (+1.33 MB, +40 crates) for sources that already run one thread each.

## Decision

- A catalog with a copy on disk is searched from that copy, fresh or not. An expired copy is reported as `expired catalog` and downloaded again after the search.
- A catalog with no copy at all downloads during the search. With a deadline set, the search stops waiting for it once every other source has answered, and reports it as still downloading. With `--timeout 0`, and in `bench`, the search still waits for every source. When only such catalogs were chosen, the deadline is the only limit.
- After a search, the catalogs it searched expired or stopped waiting for are handed to a detached `dataseek --quiet cache warm --source <ids>` child with no stdin, stdout or stderr, in its own process group on Unix, writing to the same cache directory. A mark in the cache's `warming` directory keeps later searches from starting the same download for 10 minutes. `--offline` starts none.
- `cache warm --source` downloads only the named catalogs.
- The trim evicts catalogs only once no other entry is left to evict. A search writes about 70 query files and a catalog is written once a week, so oldest first emptied the catalogs after some 30 searches under the 2,000 file cap: a cache measured at 1,972 query files held queries from only the last four minutes, and its catalogs had been evicted and downloaded again many times over.

## Consequences

- The first search on an empty cache costs what the live sources cost; the slow catalogs join from the next search on.
- A search can leave a process running for up to a minute after it exits. It writes only the cache, so a `dsk mcp` call still leaks nothing into the next call's output.
- A catalog whose background download fails is tried in the search again (still bounded by the other sources), and in the background again after 10 minutes.
- An expired catalog's results are up to a week older than a fresh download's would be, for one search.

## Evidence

Release builds on an Apple M4, 2026-10-07 and 2026-10-08, against the live sources.

| run | before | after |
|---|---|---|
| first search, empty cache, default deadline | 20.0 s, 3 catalogs missing, every time | 2.0 s, 10 catalogs downloading in the background |
| the same search once the background download finished (about 33 s) | 20.0 s | 0.25 s from the query cache, 2.3 s for a new query |
| a search with PhysioNet's catalog expired | 20.0 s until `cache warm` ran | PhysioNet answered from the expired copy in 2 ms and was downloaded again in the background |

Per-source latency on an empty cache with no deadline: median 0.55 s, 90th percentile 2.0 s; the tail was OpenNeuro 30.9 s, PhysioNet 24.1 s and ArrayExpress timing out at 15 s. With the catalogs cached, five new queries had a median source latency of about 0.2 s and a slowest source between 1.4 and 5.3 s.
