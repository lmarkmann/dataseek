# ADR 0015: `dsk mcp` is a hand-written stdio server that runs each tool as a child

- Status: accepted
- Date: 2026-10-07

## Context

Agents with a shell call `dsk search --json`. Chat apps and IDE panels without a shell reach tools only through the Model Context Protocol. The current revision, 2026-07-28, is stateless: every request names its protocol version and the client's capabilities in `_meta`, `server/discover` replaces the `initialize` handshake, and every result carries `resultType`. Most installed clients still speak the handshake revisions, 2025-11-25 and 2025-06-18. The spec lets one server serve both.

dataseek is blocking (ureq) and its release binary size is tracked (#20). Its commands also lean on process globals: `--offline` switches the HTTP client off for good, the narration mode and the cache location are set once in `main`, and a search past its deadline leaves its slow sources' threads running (ADR 0007).

## Decision

- `dsk mcp` reads newline-delimited JSON-RPC on stdin and writes one response per line on stdout, by hand, with `serde_json` and no new dependency. It serves both eras: a request naming `2026-07-28` in `_meta` is served statelessly, and `initialize` negotiates a handshake revision. One result shape serves both, since the handshake revisions allow the extra keys the stateless one requires.
- Three tools, `search`, `sources` and `inspect`. Each one's input schema is generated from the object `help <command> --json` prints, and a call's arguments become that command's flags through the same object. A flag added to a command is a tool argument at once, and clap validates it as on the command line.
- Each call runs this binary as `<command> --json <flags>` in a child process. Its stdout is the tool's structured content and text copy; its stderr events are relayed to the server's stderr; a failed run's error event becomes an `isError` result carrying the usual message, causes and `Try:` hint. `notifications/cancelled` kills the child.
- Every `search` gets an explicit `--timeout`, the argument or the flag's default, so an inherited `DATASEEK_TIMEOUT=0` cannot unbound it, and `timeout: 0` is refused.

## Consequences

- A tool's result is byte for byte what `--json` prints, and `--offline`, the narration mode and detached search threads end with the child, so no call leaks into the next.
- The server's own stdout is written only by the response writer, so it carries protocol messages and nothing else.
- Each call pays one process start, under the 20 ms startup budget, against a search that spends seconds on the network.
- Reverting to an SDK means taking on an async runtime; the protocol code to replace is `src/mcp.rs`.

## Evidence

Release builds on an Apple M4, `opt-level = 3`, thin LTO, stripped, measured 2026-10-07:

| build | binary | crates (`cargo tree -e normal`) |
|---|---|---|
| v0.6.0 | 4,938,832 bytes | 123 |
| with this server | 5,057,744 bytes (+118,912, +2.4%) | 123 |
| v0.6.0 plus `rmcp` 3.5.1 (default features and `transport-io`) serving three stub tools | 6,269,584 bytes (+1.33 MB, +27%) | 163 (+40: tokio, futures, tokio-util, tracing, schemars, uuid, ...) |
