# Troubleshoot

Verified against `docs/reference/contract.md`, `src/mcp/tools.rs`, `src/search.rs`, `src/http.rs` and live calls on 2026-10-20.

## Contents

- **Read the failure**: where the message, cause and hint sit on each surface
- **Tool refusals**: the argument errors a call can fix by itself
- **Protocol errors**: JSON-RPC codes, which mean the client or the server, not the call
- **Exit codes**: what the CLI returns and what counts as success
- **Whole-search failures**: offline, deadline, certificates, nothing could run
- **Per-source statuses**: one source missing from an otherwise good search
- **No results**: when sources answered and nothing came back

## Read the failure

Every failure carries three parts, in this order: what went wrong, why, and what to do.

- **MCP:** a result with `isError: true`. Its text is the CLI's lines, `Error: ...`, `  Cause: ...`, `  Try:   ...`; its `structuredContent` is the event `{"event": "error", "message", "causes", "try"}`.
- **CLI:** the same lines on stderr; under `--json`, one `dataseek-events/1` line with `"event": "error"`, and nothing on stdout.

Quote the lines as they are, follow `Try:` first, and rerun once. A second identical failure goes to the user with both attempts, not into a loop.

## Tool refusals

A bad argument is refused before anything runs, naming the argument. Fix the call and send it again:

| Message | Fix |
|---|---|
| `` search has no argument `<key>` `` | use a name from the tool's input schema; the names are the CLI's long flags without dashes |
| `` `<key>` takes a list of strings `` (or an integer, true or false) | send the schema's type; `source`, `exclude` and `category` are arrays |
| `` search needs `query` `` | pass `query` |
| `invalid value '<x>' for '--source <IDS>'`, with the possible values as its cause | use an id from that list or from the `sources` tool |
| `timeout 0 waits for every source, and a search over MCP always has a deadline` | pass 1 to 300, or leave `timeout` out for 20 |
| `timeout <n> is past the 300 second deadline a search over MCP may have` | 300 at most |
| `` `<key>` holds a NUL character `` | strip it |
| `4 calls are already running, the most dsk mcp runs at once` | wait for one to answer, then call again |

## Protocol errors

A JSON-RPC error instead of a tool result is the client's or the server's fault, never the call's arguments: `-32700` a line that is not JSON, `-32600` a malformed request or a reused id, `-32601` an unknown method, `-32602` an unknown tool, `-32603` a server fault whose message says what the command left, `-32022` an unsupported protocol revision with the supported list. Report it to the user with the code and message; for a tool that never appears, run the probe in `references/setup.md`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success; for `search`, at least one source answered, even with no results |
| 1 | failure: every attempted source failed, none could run, an `inspect` failed, or a `cache warm` catalog failed |
| 2 | usage error: an unknown command, flag or value |
| 130 | interrupted |

A source that fails while others answer is a warning on stderr and a `status` in the JSON, never a failure.

## Whole-search failures

When `search` itself fails, the message says which case it is:

| Case | What to do |
|---|---|
| the machine looks offline (every source unreachable: no DNS, refused connection, failed TLS handshake) | check the network; `offline: true` searches the cache and downloaded catalogs |
| `--offline` with nothing cached | go online once; `dsk cache warm` stores the catalogs for later offline use |
| no source answered before the deadline | raise `timeout` (CLI: `--timeout 0` waits for all) |
| a certificate dataseek cannot verify, on every source | on Linux, install the distribution's `ca-certificates` package, or point `SSL_CERT_FILE` at the proxy's root certificate; the `Try:` line names the variable or path at fault |
| none could run: each lacks its key, each is resting after an outage, or `source`, `category` and `exclude` left none | read the `sources` statuses; add the key, name the resting source, or widen the selection |

## Per-source statuses

The search succeeded but one source is missing. Act only when that source matters for the topic:

| `status` | Cause | Fix |
|---|---|---|
| `needs $VAR` | its required key is not set | the user sets the key (`references/setup.md`, Keys); then search again |
| `skipped, outage N s ago` | it failed within the last ten minutes | `source` set to it asks it anyway, or wait out the ten minutes |
| `not waited for after N s` | slower than the deadline, or than one second past the 90% quorum | `source` set to it gets the whole `timeout`; raise `timeout` for a slow one |
| `downloading its catalog` | a first download is running in the background | search again in a minute, or `dsk cache warm -s <id>` |
| `expired catalog` | answered from a copy over 7 days old | nothing urgent; `dsk cache warm` refreshes it |
| `stale cache (<reason>)` | the fetch failed and an older answer was used | say the answer may be dated; `refresh: true` once the source is back |
| `rate limited by the source` | too many requests from this address | wait; for `huggingface` or `github`, a key raises the limit |
| `refused the request (HTTP 401)` or `(HTTP 403)` without a key | the source wants authentication or blocks the request | for a source that takes a key, the key may lift it |
| `rejected the credentials (HTTP <n>)` | the key that was sent is wrong or expired | the user replaces the key |
| `sent a challenge page instead of results` | a bot wall; counted as an outage | retry after ten minutes |
| `returned an unexpected shape: ...` | the source changed its API | report it at https://github.com/lmarkmann/dataseek/issues with the status text |
| `unreachable (...)`, `timed out`, `answered HTTP <n>` | the source is down or slow | retry later; the other sources stand |
| `could not verify the certificate (...)` | TLS trust, as in the whole-search case | as above |
| `did not connect within --connect-timeout (N s)` | the user's own connect limit | raise or drop `--connect-timeout` in the server's args |
| `not asked (--offline)` | an offline search had nothing cached for it | search online |

## No results

An empty `results` list with sources answering means none of them holds a match for those words. In order:

1. Check `sources`: if the sources that would hold this topic did not answer, it is the table above, not the query.
2. Shorten the query to its two most specific words, then try the field's own term or acronym (`references/search.md`, Writing queries).
3. Drop `category` or `source` if they were set.
4. After three rounds, report the queries tried and the coverage, and say no dataset matched.
