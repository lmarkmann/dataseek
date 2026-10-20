---
name: dataseek
description: >
  Not for papers or citations, not for reading rows inside one dataset, not
  for downloading or converting data files, and not for changing dataseek's
  own code or source adapters. Finds datasets across 76 archives at once
  (Hugging Face, Kaggle, Zenodo, Dataverse, data.gov, data.europa.eu, IMF,
  OECD, ECB, NASA, Copernicus, GEO and more) through dataseek: its MCP
  server's search, sources and inspect tools, or the dsk CLI with --json.
  Use on "find a dataset on", "is there open data about", "where can I get
  data for", "search Kaggle and Hugging Face for", "what files and license
  does this dataset have", "add dataseek to my MCP client", "which API keys
  does dataseek need", and when a dsk command or dataseek tool call fails,
  skips a source or returns nothing.
compatibility: needs the dataseek binary (dsk) and network access
---

# dataseek

dataseek sends one query to every dataset source it knows, merges the records that are one dataset (a shared DOI, landing URL, or title and publisher) and ranks them by how many sources agree and how much of the query each covers. It runs as an MCP server (`dsk mcp`, three tools) and as a CLI (`dsk`, also installed as `dataseek`). They are one program: a tool returns, byte for byte, the object `dsk <command> --json` prints, so every reference here holds for both.

**Pick the surface once per task.** If the tool list holds the dataseek server's `search`, `sources` and `inspect` tools, call them. Otherwise, with a shell, run `dsk <command> --json`. With neither, go to the Setup lane. Clients name the tools differently (`search`, `dataseek.search`, `mcp__dataseek__search`); the server is `dataseek`.

## Output contract

Each lane ends in something the user can check without rerunning it.

Search lane:
Leaves behind: the final message, in three parts in this order
Sections: Datasets, Coverage, Queries

- **Datasets**: numbered, best first. Each carries the title, the URL and the sources that hold it, then license, last update, DOI and size when the result has them. Every value comes from the result object; a field the result lacks is left out, never guessed.
- **Coverage**: one line from the result: how many sources answered, whether `complete` was true, and each relevant source that failed, was skipped or was not waited for, with its `status` as dataseek wrote it.
- **Queries**: every query run, as the `dsk search ...` command that reproduces it.

Inspect lane:
Leaves behind: the final message, in three parts in this order
Sections: Metadata, Files, Not stated

Setup lane:
Leaves behind: the client's MCP entry for dataseek, and the count of sources one `sources` call returned
Sections: Config, Verified

Troubleshoot lane:
Leaves behind: the final message, quoting the error or status verbatim
Sections: Diagnosis, Fix, Result

Forbidden: [A-Z_]*(TOKEN|API_KEY)=[^\s$"'{]
Forbidden: (-s |--source[ =]|"source"\s*:\s*\[)[^\]\n]*\b(google|mendeley)\b
Forbidden: \bdsk (s|search|sources|inspect)\b(?:(?!--json)[^|\n])*\|

The exemptions are under Iron rules.

## Lanes

| Task smells like | Lane | Open |
|---|---|---|
| "find a dataset on", "is there data about", "on Hugging Face or Kaggle", a topic with no dataset named yet (default) | Search | `references/search.md` |
| a dataset URL handed over, "what files does it have", "what license", "how big", "what format" | Inspect | `references/inspect.md` |
| install dataseek, add it to an MCP client, "the tools do not show up", API keys, credentials | Setup | `references/setup.md` |
| an `Error:` with a `Try:` line, `isError: true`, a source skipped or not waited for, no results, offline, certificate errors | Troubleshoot | `references/troubleshoot.md` |
| a shell script, a pipeline, `jq`, exit codes, CI, many queries in a loop | Script | `references/cli.md` |

Default when ambiguous: **Search**. A search the user will choose from runs Search, then Inspect on the two or three candidates they care about. A failed call in any lane moves to Troubleshoot for that call only, then back.

## Search (default)

1. **Write one to three short queries**, two to four content words each, before calling anything. Most sources match every word, so a sentence finds fewer datasets than its keywords: "sea surface temperature" merged 328 results where "global monthly sea surface temperature dataset since 1980" merged 136 (2026-10-20). Years, regions and formats are filters to apply to the results, not query words. The rules and the evidence: `references/search.md`, Writing queries.
2. **Narrow only on the user's words.** A named archive becomes `source`; a clear domain becomes `category`; a source flooding the list becomes `exclude`. The ids and the fourteen categories: `references/search.md`, Arguments.
3. **Call `search`** and read `results`, then `complete` and `sources[].status`. An empty `results` list from sources that answered is an answer, not a failure.
4. **Reformulate when fewer than about five results fit**: a synonym, the field's own term or acronym, one word dropped. Stop after three rounds and say what was tried.
5. **Answer in the Search shape above.** Ranks and `score` order one search only; never compare scores across searches.

## Inspect

Call `inspect` with the result's landing page URL. It reads the page's schema.org Dataset markup (Croissant for Hugging Face) and lists the files the page declares; it downloads nothing. When it answers `carries no schema.org Dataset markup` and the result has a DOI, try `https://doi.org/<doi>` once, then say the page publishes no metadata and give its URL. Which archives publish markup, the JSON shape, and the answer shape: `references/inspect.md`.

## Setup

Check `dsk -V` first, then register `{"command": "dsk", "args": ["mcp"]}` under the client's MCP servers and verify with one `sources` call. Install commands, the absolute-path and `uvx` variants, the stdin handshake probe, keys and `credentials.toml`: `references/setup.md`.

## Troubleshoot

Quote the `Error:`, `Cause:` and `Try:` lines (or the source's `status`) exactly, follow the `Try:` line first, and rerun once. Every refusal, exit code and status with its fix: `references/troubleshoot.md`.

## Script

`dsk <command> --json` prints one object on stdout tagged with its `schema`; narration and errors go to stderr as `dataseek-events/1` lines. Run pipelines under `set -o pipefail`, since `jq` exits 0 on the empty stdout of a failed run. Recipes, exit codes, variables and `dsk help --json`: `references/cli.md`.

## Boundaries

- Owns: finding datasets with dataseek, reading one dataset page's metadata and file list with `inspect`, installing and registering dataseek, its keys and cache, and reading its errors.
- Does not own: papers, citations and authors (a scholarly search tool); rows, columns or statistics inside one dataset (the archive's own API, such as the Hugging Face Dataset Viewer); downloading, converting or loading files; changing dataseek's code or a source adapter (the repository's `CONTRIBUTING.md` and `docs/`).
- `dsk bench` sends queries x sources requests and skips the cache. It is a maintainer's tool: run it only when the user asks for it by name.

## Reference map

| File | Contents |
|---|---|
| `references/search.md` | Arguments on both surfaces, source ids and categories, writing queries, reading results and statuses, opt-in sources, more results |
| `references/inspect.md` | What `inspect` reads, its JSON shape, which archives publish markup, the DOI fallback, the answer shape |
| `references/setup.md` | Install, MCP registration, the handshake probe, keys and `credentials.toml`, cache warm-up |
| `references/troubleshoot.md` | Tool refusals, protocol errors, exit codes, whole-search failures, per-source statuses and their fixes |
| `references/cli.md` | Commands, global flags, variables, JSON schemas, stderr events, `jq` recipes, batches |

## Learning loop

When dataseek behaves differently from this skill, dataseek wins: read the live surface (`tools/list`, or `dsk help <command> --json`), follow it, and add one line to the final message naming the statement here that was wrong, so the user can report it at https://github.com/lmarkmann/dataseek/issues.

## Iron rules

1. **One surface per task**, MCP tools when listed, `dsk ... --json` otherwise. Never parse the text output; it wraps and truncates for a terminal.
2. **Every URL, DOI, license, size and date in an answer appears in a tool result.** dataseek returns records, not judgments about them; a field it did not return is "not stated".
3. **Google Dataset Search (`google`) and Mendeley Data (`mendeley`) only when the user names them.** Neither permits automated searches. Before a Mendeley search, tell the user its terms bar automated access without written permission.
4. **Keys never travel in an argument, a command line or a project file.** No tool takes one. They live in the environment the server starts in or in `credentials.toml` with mode 600.
5. **`complete: false` is reported, not hidden.** Name the sources it did not wait for; when one matters, search again with `source` set to it, which gives it the whole timeout.
6. **An argument you are unsure of is read from the live schema** (`tools/list`, or `dsk help search --json`), never guessed from memory; a bad argument is refused by name.
7. **At most four tool calls at once.** A fifth is refused as busy.

| Boundary | Forbidden token | Exemption |
|---|---|---|
| Rule 4, keys | a literal assignment such as `HF_TOKEN=hf_x` in a command | `export HF_TOKEN="$(<secret manager read>)"`, which carries no literal |
| Rule 3, opt-in sources | `google` or `mendeley` in `source` or `-s` | the user named Google Dataset Search or Mendeley Data in this task |
| Rule 1, text output | `dsk search ...` piped without `--json` | a human reading through a pager, `dsk search ... \| less -R` |
