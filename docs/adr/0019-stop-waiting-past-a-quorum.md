# ADR 0019: Past 90% of the sources, the rest get one second

- Status: accepted; amends [ADR 0007](0007-cache-budget-and-failure-handling.md)
- Date: 2026-10-08

## Context

ADR 0007 waits for every source up to a 20 second deadline. With catalogs searched from the cache (ADR 0018), most of the 71 default sources answer within two seconds, and a search still lasts as long as its slowest one: 3.55 s at the median and 9.57 s at the 90th percentile over the 48 relevance queries asked for the first time. The late source is a different one each time, though DataONE, whose server takes about 2.5 s on any new query, is late in two searches of three.

## Decision

- With a deadline set and no source named, once 90% of the sources asked have finished (answered or failed), a search waits at most one second longer for the rest. The share is rounded up, so a search of nine sources or fewer waits for all of them.
- Sources named with `-s` get the whole deadline: naming one says its answer is wanted. `--timeout 0` and `bench` wait for every source, as before.
- A source left behind is not a failure: the summary line says it is still running (`68 of 71 sources answered, dataone still running`), and `-v` and `--json` give it the status `still running after N s`.

## Consequences

- A default search ends within about two seconds of starting, unless fewer than 90% of its sources have answered by then, when the 20 second deadline still bounds it.
- The merged top 10 differs from waiting for all in about one slot in eleven, and nDCG@10 falls by 0.033 on the tuning half and not measurably on the held-out half. Anyone who wants every source pays for it with `--timeout 0`.
- `dsk mcp` refuses a `timeout` of 0, so an MCP client cannot ask for every source; it gets the same rule as the command line.
- Changing the share or the grace is one constant each in `src/search.rs`; `just relevance cutoff` measures a new pair against the recorded latencies before it ships.

## Evidence

[`../bench/2026-10-08-straggler-cutoff.md`](../bench/2026-10-08-straggler-cutoff.md): thirteen rules replayed against the relevance snapshot with per-source latencies measured live. Under this rule the median search falls from 3.55 s to 1.83 s, the 90th percentile from 9.57 s to 1.97 s and the slowest from 11.99 s to 2.05 s; the full top 10 keeps 91.0% of its results; nDCG@10 changes by -0.033 [-0.056, -0.013] on the tuning half and +0.002 [-0.024, +0.028] on the held-out half, and known-item MRR by +0.006 [+0.000, +0.017].
