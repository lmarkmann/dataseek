# ADR 0013: Opt-in sources are asked only when named

- Status: accepted; supersedes [ADR 0008](0008-google-dataset-search-from-page-data.md)
- Date: 2026-10-06

## Context

[ADR 0004](0004-every-source-is-searched.md) searches every source by default. That is right where a source publishes an API for clients like this one, and wrong where it does not. Google Dataset Search has no API; dataseek reads its results page (ADR 0008). Google's terms bar bypassing its protective measures and automated access that breaks the machine-readable instructions on a page, and the host serves no robots.txt, so the position rests on silence rather than on permission. With thousands of installs each sending that request on every search, silence is not enough to make it a default. Mendeley Data is the second case: its terms bar bots without written permission and its robots.txt disallows the API path the module requests (source-by-source quotes in [`../synthesis/terms.md`](../synthesis/terms.md)).

## Decision

A registry row can carry an opt-in reason. `select` in `src/sources.rs` leaves such a source out of every run unless `--source` names it; `-c`, `-x` and an empty `-s` never reach it. `dataseek sources` marks the row `opt-in` and prints the reason under the table, `--json` rows carry `opt_in` (the reason, or null), and `dataseek doctor` lists each one. Today that is Google Dataset Search and Mendeley Data.

Everything else ADR 0008 decided for Google stands, restated here because that ADR is now superseded:

- One request per query to the results page; the `ds:0` block is parsed, with field positions pinned by a fixture test.
- A consent or "unusual traffic" interstitial is `Blocked`, which counts as an outage (skipped for 10 minutes unless named).
- A page without the block, or a non-empty result list in which no record matches the known layout, is a shape error, never an empty result; the search falls back to the last cached answer.
- A record missing its title or link is skipped, not guessed at.
- No paging (start, page and offset change nothing), no retries against Google, results cached for 6 hours like every source.

## Consequences

- A default search asks 74 of the 76 registered sources. Google's results no longer appear unless asked for; the other sources' overlap is unchanged.
- `dsk search climate -s google` is the way to ask for it, and `-s google,zenodo` combines it with others.
- A source whose terms need written permission is marked opt-in, not removed, so the day permission arrives the change is one line in the registry.
- The plain `sources` output gained a seventh column, the opt-in reason, empty for every other row.

## Evidence

- 2026-10-06: `datasetsearch.research.google.com/robots.txt` answers 404; the parent `research.google.com/robots.txt` allows `/` and disallows `/app/`. The page still returns 20 results and a total hit count at `ds:0` position 8 (153 to 157 for "climate temperature"), and `start`, `page` and `offset` return the same first 20.
- Bench 2026-10-06 (ADR 0008): 6 of 6 queries answered, median 706 ms; Kaggle shared the most hits with Google (3 of 60).
- Mendeley Data: clause 3.5 of its terms and `Disallow: /api/` in its robots.txt, quoted in the terms table.
