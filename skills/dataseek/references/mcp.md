# The MCP server

Verified against a live `dsk mcp` session (`initialize`, `tools/list`, `tools/call` for all four tools, bad arguments) and `docs/reference/contract.md`, MCP mode, on 2026-11-23.

This is the primary surface. Every other reference is written in tool arguments; this one says what the server is, what a call sends and gets back, and how each call maps onto the CLI when no tool is listed.

## Contents

- **The server**: what `dsk mcp` serves and what it does not
- **The four tools**: every argument, its type and its bounds
- **A call and its result**: the arguments object, and where the answer sits in the result
- **A failed call**: the two kinds of failure and what each one means
- **Limits**: four calls at once, the deadline, cancellation, lifetime
- **What only the CLI does**: the commands with no tool
- **The CLI fallback**: turning a tool call into the `dsk ... --json` command that returns the same object
- **Server options**: the flags after `mcp` that change every call
- **An older server**: a tool list without `related`, or fewer sources

## The server

`dsk mcp` is a local MCP server on stdio. It serves tools only: four of them, `search`, `sources`, `inspect` and `related`. It has no resources and no prompts; `resources/list` and `prompts/list` answer `-32601`. The server names itself `dataseek`, and a client shows its tools under its own naming scheme: `search`, `dataseek.search`, `mcp__dataseek__search`.

Each tool runs one CLI command with `--json` and returns the object that command prints. A fact about a result in any reference here therefore holds for the tool and for the command.

The server needs nothing but the binary: no account, no daemon, no port. It keeps no state between calls except the on-disk cache, which the CLI shares.

## The four tools

| Tool | Does | Arguments | Result schema |
|---|---|---|---|
| `search` | asks the sources, merges the records that are one dataset, ranks them | `query` (required) and 14 optional ones | `dataseek-search/1` |
| `sources` | lists every source with its category, protocol, key status and docs | none | `dataseek-sources/1` |
| `inspect` | reads one dataset page's metadata and the files it declares | `url` (required), `offline` | `dataseek-inspect/1` |
| `related` | lists the datasets beside one item, from the item's own host | `url` (required), `by`, `limit`, `offline` | `dataseek-related/1` |

Every schema sets `additionalProperties: false`, so an argument the tool does not have is refused by name, never ignored.

`search` arguments, as `tools/list` gives them:

| Argument | Type | Bounds and default |
|---|---|---|
| `query` | string | required; several words form one query |
| `source`, `include`, `exclude` | array of strings | each item is one of the 106 source ids; the schema lists them as an `enum` |
| `category` | array of strings | each item is one of the 14 category names, also an `enum` |
| `license`, `format` | array of strings | free text: SPDX ids or a family such as `cc-by`; format names such as `csv` |
| `updated-since` | string | pattern `^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$`: `2024`, `2024-07` or `2024-07-01` |
| `min-sources` | integer | at least 1 |
| `limit` | integer | default 20 |
| `per-source` | integer | 1 to 100; 10 by default, 30 when a filter is set |
| `sort` | string | `relevance` (default) or `newest` |
| `timeout` | integer | 1 to 300 seconds, default 20 |
| `refresh`, `offline` | boolean | false by default |

What each argument is for, and when to set it: `references/search.md`, Arguments. The arguments of `inspect` and `related`: `references/inspect.md` and `references/related.md`.

Argument names keep the CLI's hyphen: `updated-since`, `min-sources`, `per-source`. An underscore form is refused as an unknown argument.

## A call and its result

A client sends the arguments as one JSON object:

```json
{ "query": "sea surface temperature", "category": ["geospatial"], "limit": 10 }
```

```json
{ "query": "mnist", "source": ["huggingface", "kaggle"] }
```

```json
{ "url": "https://zenodo.org/records/13135140" }
```

The result has five keys: `content`, `structuredContent`, `isError`, `resultType` and `_meta`.

- **`structuredContent`** is the object itself, tagged with its `schema`. Read this when the client exposes it.
- **`content[0].text`** is the same object as one line of JSON. A client that shows only text gives this; parse it as JSON, do not read it as prose.
- **`isError`** is false for an answer and true for a failed call.

An empty `results` list with `isError: false` is an answer: the sources that answered hold no match.

## A failed call

There are two kinds, and they mean different things.

**A tool failure** is a normal result with `isError: true`. The call was understood and could not finish, or its arguments were wrong. Its text is three lines, and its `structuredContent` is the same failure as an event:

```text
Error: `source` takes a list of strings
  Try:   tools/list shows the type of each argument
```

```json
{ "schema": "dataseek-events/1", "event": "error",
  "message": "`source` takes a list of strings", "causes": [],
  "try": "tools/list shows the type of each argument" }
```

A tool failure is yours to fix: read `try`, change the call, send it again once. A bad argument, a missing `query`, a page `inspect` cannot read and a fifth concurrent call all arrive this way. Each message and its fix: `references/troubleshoot.md`.

**A protocol error** is a JSON-RPC `error` with a code and no result. The fault is in the client or the server, not in your arguments: `-32601` an unknown method, `-32602` an unknown tool or `arguments` that is not an object, `-32603` a server fault, `-32700` a line that is not JSON, `-32600` a malformed request or a reused id, `-32022` an unsupported protocol revision. Give the code and message to the user.

## Limits

- **Four calls at once.** A fifth is refused with `4 calls are already running, the most dsk mcp runs at once`. Wait for one answer, then send it. Independent queries run well in parallel, up to four.
- **Every search has a deadline.** `timeout` is 1 to 300 seconds, 20 when left out; 0 is refused. Past the point where 90% of the sources asked have answered, the others get one more second. A source named in `source` is exempt and gets the whole `timeout`. This is why a default search ends in a few seconds with `complete: false` and some sources `not waited for`.
- **Cancellation.** A client's `notifications/cancelled` kills that one call, which then gets no answer.
- **Lifetime.** Closing the server's stdin ends it at once and kills every call still running. A running server keeps the binary it started as, so an upgrade takes effect only after the client restarts the server.
- **Narration goes to stderr.** The server writes its status line and each call's progress there, as `dataseek-events/1` lines. Clients show it in their MCP logs; it never reaches a tool result.

## What only the CLI does

No tool runs these. When the task needs one and a shell is at hand, run the command; otherwise give it to the user.

| Need | Command |
|---|---|
| download the local catalogs now | `dsk cache warm` |
| see the cache path, size and budget | `dsk cache info` |
| delete the cache | `dsk cache clear` (show `--dry-run` and ask first) |
| see where each key was found, and the config and cache paths | `dsk doctor` |
| wait for every source, however slow | `dsk search ... --timeout 0` |
| read the whole surface as JSON | `dsk help --json` |

## The CLI fallback

When the tool list holds no dataseek tools and a shell has `dsk`, run the command that returns the same object. The rule is mechanical:

| In the tool call | On the command line |
|---|---|
| the tool name | the command: `dsk search`, `dsk sources`, `dsk inspect`, `dsk related` |
| `query` | the words after `search` |
| `url` | the URL after `inspect` or `related` |
| an array argument, `"source": ["zenodo", "figshare"]` | the long flag with comma-separated values, `--source zenodo,figshare` |
| a string or integer argument, `"limit": 10` | the long flag and the value, `--limit 10` |
| a boolean argument set to true, `"refresh": true` | the bare flag, `--refresh` |
| nothing | `--json`, always, and `-q` to keep stderr to errors |

```sh
dsk search sea surface temperature --category geospatial --limit 10 --json -q
dsk search mnist --source huggingface,kaggle --json -q
dsk inspect https://zenodo.org/records/13135140 --json -q
dsk related https://huggingface.co/datasets/nyu-mll/glue --json -q
dsk sources --json
```

Three differences from a tool call:

1. **stdout holds the object, stderr holds the failure.** A failed command prints nothing on stdout and the `Error:`, `Cause:`, `Try:` lines on stderr, with exit code 1 or 2. Check the exit code before parsing stdout.
2. **No four-call limit, no 300-second cap.** The CLI takes `--timeout 0`, wait for every source. Still run searches one after another; each one already fans out to every source.
3. **Never the text output.** Without `--json` the CLI prints a table for a terminal, wrapped and cut to its width.

Use one surface for the whole task. Scripts, pipelines and `jq` recipes: `references/cli.md`.

## Server options

Flags after `mcp` in the client's entry reach every call:

| Entry `args` | Effect |
|---|---|
| `["mcp"]` | the default |
| `["mcp", "--include", "google"]` | adds those sources, opt-in ones included, to every search that sets neither `source` nor `include` |
| `["mcp", "--cache-dir", "/path"]` | moves the cache |
| `["mcp", "--connect-timeout", "5"]` | limits how long a connection may take |
| `["mcp", "-q"]` or `["mcp", "-v"]` | less or more narration on stderr |

When a result's `sources` list holds `google` or `mendeley` and the call named neither, the user's entry carries `--include`. That is their configuration; rule 3 in `SKILL.md` covers only what a call names.

The server answers three protocol revisions: `2025-11-25` and `2025-06-18` to a client that opens with `initialize`, and `2026-07-28` statelessly. A client chooses; a call does not.

## An older server

The tool list is the truth about the server that is running. Two signs of an older release:

- **No `related` tool.** The release predates it. Search for the owner's name or the item's title instead, and tell the user that an upgrade adds the tool.
- **Fewer than 106 rows from `sources`.** The release has fewer sources. An id the server does not know is refused with the list of ids it does know; use that list.

The fix for both is in `references/setup.md`: upgrade the binary, then restart the server in the client.
