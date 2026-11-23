---
name: dataseek
description: >
  Not for papers or citations, not for reading rows inside one dataset, not
  for downloading or converting data files, and not for changing dataseek's
  own code or source adapters. Finds datasets across 106 archives at once
  (Hugging Face, Kaggle, Zenodo, Dataverse, data.gov, data.europa.eu, IMF,
  OECD, ECB, NASA, Copernicus, GEO and more) through dataseek: its MCP
  server's search, sources, inspect and related tools, or the dsk CLI with
  --json when no tool is listed.
  Use on "find a dataset on", "is there open data about", "where can I get
  data for", "search Kaggle and Hugging Face for", "which archive holds",
  "what files and license does this dataset have", "what else does this
  owner publish", "add dataseek to my MCP client", "which API keys does
  dataseek need", and when a dsk command or dataseek tool call fails, skips
  a source or returns nothing.
compatibility: needs the dataseek binary (dsk) and network access
---

# dataseek

dataseek sends one query to every default dataset source (the opt-in ones only when named), merges the records that are one dataset (a shared DOI, landing URL, or title and publisher) and ranks them by how many sources agree and how much of the query each covers. It runs as an MCP server (`dsk mcp`, four tools) and as a CLI (`dsk`, also installed as `dataseek`). They are one program: a tool returns the object `dsk <command> --json` prints, so every reference here holds for both.

**MCP first; the CLI is the fallback.** Every procedure here is written in tool arguments. Decide the surface once, at the start, and keep it for the whole task:

1. **The tool list holds dataseek's `search`, `sources`, `inspect` and `related`:** call them. Clients name them differently (`search`, `dataseek.search`, `mcp__dataseek__search`); the server is `dataseek`. What a call sends and returns: `references/mcp.md`.
2. **No dataseek tools, but a shell with `dsk`:** run `dsk <command> --json`, built from the same arguments by the rule in `references/mcp.md`, The CLI fallback. Tell the user once that registering the server (Setup lane) turns this into tool calls.
3. **Neither:** go to the Setup lane first.

The running server decides what exists. A tool list without `related`, or a `sources` result with fewer rows than this skill names, is an older release: work with what it lists.

## Output contract

Each lane ends in something the user can check without rerunning it.

Search lane:
Leaves behind: the final message, in three parts in this order
Sections: Datasets, Coverage, Queries

- **Datasets**: numbered, best first. Each carries the title, the URL and the sources that hold it, then license, last update, DOI and size when the result has them. Every value comes from the result object; a field the result lacks is left out, never guessed.
- **Coverage**: one line from the result: how many of `total` matches are shown, how many of the sources asked answered, whether `complete` was true, and each source that matters for the topic and failed, was skipped or was not waited for, with its `status` as dataseek wrote it.
- **Queries**: every query run, as the `dsk search ...` command that reproduces it.

Sources lane:
Leaves behind: the final message, in two parts in this order
Sections: Sources, Checked

Inspect lane:
Leaves behind: the final message, in three parts in this order
Sections: Metadata, Files, Not stated

Related lane:
Leaves behind: the final message, in two parts in this order
Sections: Neighbors, Source

Setup lane:
Leaves behind: the client's MCP entry for dataseek, and the count of sources one `sources` call returned
Sections: Config, Verified

Troubleshoot lane:
Leaves behind: the final message, quoting the error or status verbatim
Sections: Diagnosis, Fix, Result

Forbidden: [A-Z_]*(TOKEN|API_KEY)=[^\s$"'{]
Forbidden: (-s |--source[ =]|--include[ =]|"(source|include)"\s*:\s*\[)[^\]\n]*\b(google|mendeley)\b
Forbidden: \bdsk (s|search|sources|inspect|related)\b(?:(?!--json)[^|\n])*\|

The exemptions are under Iron rules.

## Lanes

| Task smells like | Lane | Open |
|---|---|---|
| "find a dataset on", "is there data about", "on Hugging Face or Kaggle", a topic with no dataset named yet (default) | Search | `references/search.md`; `references/sources.md` to choose sources |
| "which archives does it search", "does it cover X", "which source holds this kind of data", "is my key set" | Sources | `references/sources.md` |
| a dataset URL handed over, "what files does it have", "what license", "how big", "what format" | Inspect | `references/inspect.md` |
| a dataset, owner, paper, model or repository URL with "what else did they publish", "which datasets use this paper or model", "its forks" | Related | `references/related.md` |
| install or upgrade dataseek, add it to an MCP client, "the tools do not show up", API keys, credentials | Setup | `references/setup.md` |
| an `Error:` with a `Try:` line, `isError: true`, a source skipped or not waited for, no results, offline, certificate errors | Troubleshoot | `references/troubleshoot.md` |
| a shell script, a pipeline, `jq`, exit codes, CI, many queries in a loop | Script | `references/cli.md` |

Default when ambiguous: **Search**. A search the user will choose from runs Search, then Inspect on the two or three candidates they care about. A failed call in any lane moves to Troubleshoot for that call only, then back. Any question about the call itself (an argument's type, the result envelope, the four-call limit, the CLI form of a call) is answered by `references/mcp.md`.

Do not read `evals/` during normal work. Open it only when testing or changing this skill.

## Search (default)

1. **Write one to three short queries**, two to four content words each, before calling anything. Most sources match every word, so a sentence finds fewer datasets than its keywords: "sea surface temperature" merged 345 results where "global monthly sea surface temperature dataset since 1980" merged 187 (2026-11-23). Plain words only, no operators. Licenses, dates and formats go in the `license`, `updated-since` and `format` arguments, not the query. The rules and the evidence: `references/search.md`, Writing queries.
2. **Narrow only on the user's words.** "Only on Kaggle" becomes `source` (those alone); "also check Kaggle" becomes `include` (beside every default source); a clear domain becomes `category`; a source flooding the list becomes `exclude`. The ids by category: `references/search.md`, Source ids and categories. Which sources fit a need: `references/sources.md`.
3. **Call `search`** and read `results`, then `total`, `complete` and `sources[].status`. A `results` list as long as `limit` with a larger `total` means more exist. An empty `results` list from sources that answered is an answer, not a failure.
4. **Ask a slow source by name when the topic is its own.** A default search stops one second after 90% of the sources answer, so slow archives are `not waited for`. A second call with `source` set to them and a higher `timeout` gets them: the four EBI archives need `timeout: 150`.
5. **Reformulate when fewer than about five results fit**: a synonym, the field's own term or acronym, one word dropped. Stop after three rounds and say what was tried.
6. **Answer in the Search shape above.** Ranks and `score` order one search only; never compare scores across searches.

## Sources

Call `sources` and answer from its rows: `id`, `name`, `category`, `protocol`, `search` (`live`, or `local` for a list dataseek downloads and searches itself), `key`, `key_env`, `opt_in` and `notice`. **Sources** lists the ones that fit the question with what each holds; **Checked** is one line with the call made and how many rows it returned. What each of the 106 holds, what one record is at each, and how each matches a query: `references/sources.md`.

## Inspect

Call `inspect` with the result's landing page URL. It reads the page's schema.org Dataset markup (Croissant for Hugging Face) and lists the files the page declares; it downloads nothing. When it answers `carries no schema.org Dataset markup` and the result has a DOI, try `https://doi.org/<doi>` once, then say the page publishes no metadata and give its URL. Which archives publish markup, the JSON shape, and the answer shape: `references/inspect.md`.

## Related

Call `related` with the page of an item already in hand: a Hugging Face dataset, owner, paper, model or collection page, an arXiv link, a GitHub repository or owner page, or a GitLab project page. It asks that one host for the item's neighbors and returns them as search records, in the host's order. Set `by` to `forks` for the forks of a GitHub repository. The pages, the relations, the limits and the answer shape: `references/related.md`.

## Setup

Check `dsk -V` first, then register `{"command": "dsk", "args": ["mcp"]}` under the client's MCP servers and verify with one `sources` call. Install and upgrade commands, the absolute-path and `uvx` variants, the stdin handshake probe, keys and `credentials.toml`: `references/setup.md`.

## Troubleshoot

Quote the `Error:`, `Cause:` and `Try:` lines (or the source's `status`) exactly, follow the `Try:` line first, and rerun once. Every refusal, exit code and status with its fix: `references/troubleshoot.md`.

## Script

`dsk <command> --json` prints one object on stdout tagged with its `schema`; narration and errors go to stderr as `dataseek-events/1` lines. Run pipelines under `set -o pipefail`, since `jq` exits 0 on the empty stdout of a failed run. Recipes, exit codes, variables and `dsk help --json`: `references/cli.md`.

## Boundaries

- Owns: finding datasets with dataseek, saying which sources it covers and what each holds, reading one dataset page's metadata and file list with `inspect`, listing the datasets beside one item with `related`, installing and registering dataseek, its keys and cache, and reading its errors.
- Does not own: papers, citations and authors (a scholarly search tool); rows, columns or statistics inside one dataset (the archive's own API, such as the Hugging Face Dataset Viewer); downloading, converting or loading files; changing dataseek's code or a source adapter (the repository's `CONTRIBUTING.md` and `docs/`).
- `dsk bench` sends queries x sources requests and skips the cache. It is a maintainer's tool: run it only when the user asks for it by name.

## Reference map

| File | Contents |
|---|---|
| `references/mcp.md` | The server, the four tools and their schemas, a call and its result, tool failures and protocol errors, limits, what only the CLI does, the CLI fallback rule, server options, an older server |
| `references/search.md` | Arguments, filters, source ids by category, writing queries, reading results and statuses, opt-in sources, more results, the answer |
| `references/sources.md` | The fields of a `sources` row, sources for a need, what each of the 106 holds, what one record is, how each matches, local catalogs and their sizes, slow, limited and keyed sources, notices |
| `references/inspect.md` | What `inspect` reads, its JSON shape, which archives publish markup, the DOI fallback, the answer shape |
| `references/related.md` | The pages `related` reads, its relations, its JSON shape, its limits, the answer shape |
| `references/setup.md` | Install, upgrade, MCP registration, the handshake probe, keys and `credentials.toml`, cache warm-up |
| `references/troubleshoot.md` | Tool refusals, a missing tool or source, protocol errors, exit codes, whole-search failures, per-source statuses and their fixes |
| `references/cli.md` | Commands, global flags, variables, JSON schemas, stderr events, `jq` recipes, batches |

## Learning loop

This skill keeps no learned file. When dataseek behaves differently from a statement here, follow dataseek (rule 6) and add one line to the final message that names the statement, so the user can report it at https://github.com/lmarkmann/dataseek/issues. The fix then lands in the reference that owns the subject.

## Iron rules

1. **MCP tools when listed, `dsk ... --json` only when they are not**, and one surface for the whole task. Never parse the CLI's text output; it wraps and truncates for a terminal.
2. **Every URL, DOI, license, size and date in an answer appears in a tool result.** dataseek returns records, not judgments about them; a field it did not return is "not stated".
3. **Google Dataset Search (`google`) and Mendeley Data (`mendeley`) only when the user names them.** Neither permits automated searches. Before a Mendeley search, tell the user its terms bar automated access without written permission.
4. **Keys never travel in an argument, a command line or a project file.** No tool takes one. They live in the environment the server starts in or in `credentials.toml` with mode 600.
5. **`complete: false` is reported, not hidden.** Name the sources it did not wait for that matter for the topic; when one matters, search again with `source` set to it, which gives it the whole timeout.
6. **The live surface wins.** An argument, a source id or a count you are unsure of is read from the running server (`tools/list`, the `sources` tool, or `dsk help search --json`), never from memory and never from this skill against the server's word. A bad argument is refused by name.
7. **At most four tool calls at once.** A fifth is refused as busy.

| Boundary | Forbidden token | Exemption |
|---|---|---|
| Rule 4, keys | a literal assignment such as `HF_TOKEN=hf_x` in a command | `export HF_TOKEN="$(<secret manager read>)"`, which carries no literal |
| Rule 3, opt-in sources | `google` or `mendeley` in `source`, `include`, `-s` or `--include` | the user named Google Dataset Search or Mendeley Data in this task |
| Rule 1, text output | `dsk search ...` piped without `--json` | a human reading through a pager, `dsk search ... \| less -R` |
