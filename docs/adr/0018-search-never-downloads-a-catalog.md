# ADR 0018: A search never downloads a catalog

- Status: accepted; supersedes the deadline paragraph of [ADR 0007](0007-cache-budget-and-failure-handling.md) on catalog downloads, detached search threads and what a deadline cut-off means for the next search
- Date: 2026-10-08

## Context

A search prints at `min(T, max_i L_i)`: `search::run` waits until every spawned thread has reported or `T` passes, and `T` is `--timeout` (default 20 s). One slow member sets the clock, and `Source::local` hid two cost classes behind one call. The probe of the catalog cache is a disk read of milliseconds; the download behind the same call is 25 to 90 s, because catalogs are whole lists (PhysioNet 35 s, OpenNeuro 25 s). So a cold or expired catalog pinned every cold search to the deadline and the run reported that download as still running, with the stderr summary pointing at `cache warm`. A thread that finished a page after the deadline could even record its host for ten minutes of skips, and find.rs kept a printing case just for those catalog-only stragglers. The CPU stages behind the barrier are already measured small: loading a 16,000-entry catalog costs 2.81 ms and searching it 3.15 ms; merging 6,080 hits is 6.82 ms (2026-10-06 pass). The wild cost is the download that sits between a live request and a disk read without declaring itself.

## Decision

- **Admission.** `search` admits a catalog source from disk alone: a fresh copy this release wrote is searched inside the barrier (a few ms); a copy past its TTL, or one written by another release, is served as `Status::Outdated(age)` and counted in the closing summary; a missing copy is `Status::NeedsWarm`: not attempted, named in the closing summary, left to `dataseek cache warm`, which downloads all catalogs, joins its threads, and keeps no deadline as before. `--refresh` still does not apply to catalogs.
- **Named sources download.** `--source` is the exception: naming a source asks for it alone, so `Plan` carries the ids and a named catalog downloads in the barrier exactly as before, deadline still capping it. `bench` and the relevance snapshot recorder name their sources too: they measure the source's own answers, not the cache's luck.
- **The deadline stops pages.** The stop flag from the same change (`Services::stop`, set at the deadline): every adapter that pages checks it before its next page and answers [`SourceError::Stopped`], never an outage. A cut-short list is therefore never cached as a full answer (the same rule the adapter page failures already keep), a host cut off by the deadline is not parked for ten minutes, and an answer a source completed on its own is cached as before.
- **Honest counts.** `Cache::has_catalog` checks that the file exists without parsing it, so the opening line and the progress bar count only sources that will be asked.

## Consequences

- On a cold catalog cache a search returns at the live tail instead of the deadline: about 8 s where ADR 0007 measured 20.3 s, with the dropped catalogs named and pointed at `cache warm`. On a warm cache nothing moves during the search (same 8 s) until a copy expires, then the copy keeps serving, labeled `outdated catalog (N d old)`.
- The once-per-search trim evicts every query entry before any catalog. Oldest first alone would delete the copies `cache warm` wrote once a few dozen searches' newer query files pushed the directory past its budget, and no search would fetch them back.
- A search never refreshes a catalog by itself anymore: only `cache warm` moves the copy forward. A user who never warms keeps searching outdated copies, with the age on the label in `-v` and `--json` and a count of them in every closing summary.
- A release no longer costs the catalogs: the previous release's copies are served as outdated, offline too, where treating them as missing would drop all 26 from every search after each upgrade. The cost is that a release fixing a catalog parser shows the old parse until `cache warm`; a copy the new `Dataset` shape cannot read is missing and waits for `cache warm`.
- `Status::Running` remains for live stragglers and named-catalog downloads only; the find.rs printing case for downloading catalogs narrows to those. A search whose every member is a cold catalog exits `1`: `every chosen source's catalog is missing from disk; run cache warm, then search again` online, the `--offline` nothing-cached error when offline. When keyless or resting sources are among them, the key or outage error leads and the summary still names the cold catalogs.
- The two concurrency models collapse into one per command: `cache warm` downloads and joins; `search` does not download. A search a deadline cut off leaves only live stragglers, and they stop before their next page: ADR 0015's detached-thread coupling shrinks to the process globals its motion to `Http` and narration arguments has still to remove, so the MCP child per call stays.
- The ranker, dedup and `catalog.rs` are untouched: results from a warm search are byte identical.

## Evidence

- ADR 0007's run: cold without the deadline 35.5 s bounded by PhysioNet, 20.3 s with it, warm about 8 s fresh with live hosts the slowest members; `cache warm` fetched 26 catalogs in about 30 s.
- 2026-10-06 pass ([bench](../bench/2026-10-06-algorithmic-speedups.md)): `catalog/load/16000` 2.81 ms, `catalog/search/16000` 3.15 ms, `merge/6080` 6.82 ms: the disk-side class is meters of milliseconds, the download class is tens of seconds.
- The admission cases are unit tested against a scratch cache (search.rs: fresh served, expired and foreign-release served as outdated, missing left to `cache warm`, named catalog downloading and falling back), the catalog probe in cache.rs tests, and the deadline stop in search.rs tests (the flag set, no outage mark, a completed answer still cached). The new end-to-end cold wall time is not re-measured on this change; the mechanism's units are the tests above, and ADR 0007's numbers bound the before.
