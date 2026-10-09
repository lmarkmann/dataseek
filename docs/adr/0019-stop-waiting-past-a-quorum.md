# ADR 0019: Past 90% of the sources, the rest get one second

- Status: accepted; amends [ADR 0007](0007-cache-budget-and-failure-handling.md)
- Date: 2026-10-08

## Context

ADR 0007 waits for every source up to a 20 second deadline. With catalogs searched from the cache (ADR 0018), most of the 71 default sources answer within two seconds, and a search still lasts as long as its slowest one: 3.55 s at the median and 9.57 s at the 90th percentile over the 48 relevance queries asked for the first time. The late source is a different one each time, though DataONE, whose server takes about 2.5 s on any new query, is late in two searches of three.

## Decision

- With a deadline set and no source named, once 90% of the sources waited for have answered, counted out of those that have not failed, a search waits at most one second longer for the rest. The share is rounded up, so a search of nine sources or fewer waits for all of them. Failures do not count toward the quorum, so a network that fails most sources quickly does not cut the one that would have answered. A catalog's first download, which the search does not wait for (ADR 0018), is not counted either.
- Sources named with `-s` get the whole deadline: naming one says its answer is wanted. `--timeout 0` and `bench` wait for every source, as before.
- A source left behind is not a failure, and its answer is discarded: the summary line names it (`68 of 71 sources answered, dataone not waited for`), `-v` and `--json` give it the status `not waited for after N s`, and `--json` sets `complete` to false.

## Consequences

- A default search ends about a second after 90% of its sources have answered, unless that never happens, when the 20 second deadline still bounds it.
- A source left behind is told so: the search sets a stop flag, and every adapter that pages checks it before its next page and gives up with an error that never marks the host as down and is never cached as an answer. The same flag is set at the deadline and when only first catalog downloads are left, so nothing the search stopped waiting for keeps paging until the process exits.
- The merged top 10 differs from waiting for all in about one slot in eleven, and nDCG@10 falls by 0.033 on the tuning half and not measurably on the held-out half. Anyone who wants every source pays for it with `--timeout 0`.
- `dsk mcp` refuses a `timeout` of 0, so an MCP client cannot ask for every source; it gets the same rule as the command line.
- Changing the share or the grace is one constant each in `src/search.rs`; `just relevance cutoff` replays a new pair, added to `RULES` in `examples/relevance/cutoff.rs`, against the recorded latencies before it ships.

## Evidence

[`../bench/2026-10-08-straggler-cutoff.md`](../bench/2026-10-08-straggler-cutoff.md): thirteen rules replayed against the relevance snapshot with per-source latencies measured live. Under this rule the median search falls from 3.55 s to 1.83 s, the 90th percentile from 9.57 s to 1.97 s and the slowest from 11.99 s to 2.05 s; the full top 10 keeps 91.2% of its results; nDCG@10 changes by -0.033 [-0.057, -0.012] on the tuning half and +0.001 [-0.016, +0.018] on the held-out half, and known-item MRR by +0.006 [+0.000, +0.017].

It is not the fastest rule with that loss: 80% plus one second ends 0.2 s sooner and leaves out 1.46 answering sources a search against 1.10. A fixed two-second deadline scores the same as this rule on the recorded connection, but it would cut every slower source on a slower connection, where a quorum waits for most of them; that argument is not something one recording measures.
