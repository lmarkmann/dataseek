# ADR 0008: Google Dataset Search is read from its results page data

- Status: superseded by [ADR 0013](0013-opt-in-sources.md)
- Date: 2026-10-06

## Context

Google Dataset Search has no API, documented or announced. Its results page embeds the first 20 results as one JSON array in an `AF_initDataCallback({key: 'ds:0', ...})` block, the data the page itself renders. The lost Python version read Google Dataset Search the same way. The paid alternative (DataForSEO's Google Dataset Search endpoint) would put a third party's scraping and a billing account between the user and the results.

## Decision

`src/sources/google.rs` sends one request per query to the results page and parses the `ds:0` block. Field positions (title, provider, URL, DOI, description, update date, file size) were mapped on 2026-10-06 and are pinned by a fixture test. Failsafes:

- A consent or "unusual traffic" interstitial is reported as blocked, which counts as an outage (skipped for 10 minutes).
- A page without the block, or a non-empty result list in which no record matches the known layout, is a shape error, never an empty result; the search falls back to the last cached answer.
- A record missing its title or link is skipped, not guessed at.
- No paging, no retries against Google, results cached for 6 hours like every source.

## Consequences

- When Google changes the page, `-v` shows `google: returned an unexpected shape` and every other source carries on. The fix is updating the positions in one function and its fixture.
- Google's results merge with the sources it indexes; Kaggle shared the most hits with it (3 of 60 across the bench).
- Moving to DataForSEO later would replace one module and add a key; nothing else changes.

## Evidence

- Probed 2026-10-06: `ds:0` held 20 results and a total of 151 for "climate temperature"; the dataseek User-Agent was served the same page as a browser.
- Bench: 6 of 6 queries answered, median 706 ms.
