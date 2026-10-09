# ADR 0015: `dsk mcp` is a hand-written stdio server that runs each tool as a child

- Status: accepted
- Date: 2026-10-07

## Context

Agents with a shell call `dsk search --json`. Chat apps and IDE panels without a shell reach tools only through the Model Context Protocol. The current revision, 2026-07-28, is stateless: every request names its protocol version and the client's capabilities in `_meta`, `server/discover` replaces the `initialize` handshake, and every result carries `resultType`. Most installed clients still speak the handshake revisions, 2025-11-25 and 2025-06-18. The spec lets one server serve both.

dataseek is blocking (ureq) and its release binary size is tracked (#20). Its commands also lean on process globals: `--offline` switches the HTTP client off for good, the narration mode and the cache location are set once in `main`, and a search past its deadline leaves its slow sources' threads running (ADR 0007).

## Decision

- `dsk mcp` reads newline-delimited JSON-RPC on stdin and writes one response per line on stdout, by hand, with `serde_json` and no new dependency. It serves both eras: a request naming `2026-07-28` in `_meta` is served statelessly, and `initialize` negotiates a handshake revision. One result shape serves both, since the handshake revisions allow the extra keys the stateless one requires.
- Three tools, `search`, `sources` and `inspect`. Each one's input schema is generated from the object `help <command> --json` prints, and a call's arguments become that command's flags through the same object. A flag added to a command is a tool argument at once, and clap validates it as on the command line.
- Each call runs the binary the server was started as, `<command> --json <flags>`, in a child process, at most four at a time. Its stdout is the tool's structured content and text copy; its stderr events are relayed to the server's stderr; a failed run's error event becomes an `isError` result carrying the usual message, causes and `Try:` hint. `notifications/cancelled` kills the child.
- Every `search` gets an explicit `--timeout`, the argument or the flag's default, so an inherited `DATASEEK_TIMEOUT=0` cannot unbound it, and a `timeout` of 0 or over 300 seconds is refused.

## Consequences

- A tool's result is byte for byte what `--json` prints, and `--offline`, the narration mode and detached search threads end with the child, so no call leaks into the next. Since [ADR 0018](0018-search-never-downloads-a-catalog.md) only live stragglers outlive a deadline and they stop before their next page, so the child now isolates `--offline`, the narration mode and the cache location alone; moving those onto construction arguments would make the child optional.
- The server's own stdout is written only by the response writer, so it carries protocol messages and nothing else.
- Each call pays one process start, under the 20 ms startup budget, against a search that spends seconds on the network.
- Moving to an SDK means taking on an async runtime; the protocol code to replace is `src/mcp.rs`.

## Evidence

Release builds of `dataseek` on an Apple M4 under the current release profile (`opt-level = 3` for dataseek and `"z"` for dependencies, fat LTO, one codegen unit, stripped), measured 2026-10-07. "Without the server" is the same tree before `dsk mcp` was added. Crates are the unique packages `cargo tree -e normal` lists for the build host; across every target (`--target all`) both builds have 125.

| build | binary | crates |
|---|---|---|
| without the server | 2,415,088 bytes | 82 |
| with the server | 2,565,632 bytes (+150,544, +6.2%) | 82 |

The SDK was measured once, against v0.6.0 under the release profile of that time (`opt-level = 3`, thin LTO, stripped), and the comparison is kept as it was taken, relative to v0.6.0:

| build | binary | crates |
|---|---|---|
| v0.6.0 | 4,938,832 bytes | 123 |
| v0.6.0 plus `rmcp` 3.5.1 (default features and `transport-io`) serving three stub tools | 6,269,584 bytes (+1.33 MB, +27%) | 163 (+40: tokio, futures, tokio-util, tracing, schemars, uuid, ...) |
