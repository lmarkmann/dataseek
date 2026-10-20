# CLI and scripts

Verified against `docs/reference/contract.md`, `dsk help --json`, `dsk help environment`, `dsk help exit-codes` and live runs on 2026-10-20. Every `jq` recipe below was run against real output that day.

## Contents

- **Commands**: which command does what, and which ones change state
- **Flags and variables**: the global flags, the `DATASEEK_*` variables and their precedence
- **Streams and schemas**: what reaches stdout, what reaches stderr, and the schema tags
- **Recipes**: `jq` for the questions scripts ask most
- **Batches**: many queries, one merged list, without hammering the sources
- **Discovering the surface**: reading flags from the binary instead of from memory

## Commands

| Command | Does | State it changes |
|---|---|---|
| `dsk search <words>` (alias `dsk s`) | the merged, ranked search | the query cache |
| `dsk sources` | every source: id, category, protocol, key status, docs | none |
| `dsk inspect <url>` | one page's metadata and file list | none |
| `dsk doctor` | config, cache and state paths, color, every key's status | none |
| `dsk cache info` | cache path, size and budget | none |
| `dsk cache warm [-s ids] [--dry-run]` | download the catalogs now, with no deadline | the catalog cache |
| `dsk cache clear [--dry-run]` | delete every cached answer and catalog | deletes the cache; show `--dry-run` and ask first |
| `dsk mcp` | the MCP server on stdio | none |
| `dsk bench [queries]` | time every source; sends queries x sources requests, uncached | none, but heavy on the sources; only when asked by name |
| `dsk help [command \| environment \| exit-codes]` | help; `--json` describes the whole surface | none |
| `dsk completion <shell>`, `dsk man` | a completion script, the man page | none |

## Flags and variables

Global flags, valid with every command: `--json`, `-q`/`--quiet` (errors only on stderr), `-v`/`--verbose` (repeatable), `--color auto|always|never`, `--no-color`, `--plain` (ASCII, no color), `--no-progress`, `--cache-dir DIR`, `--connect-timeout SECS`.

`search` flags: `-s/--source`, `-x/--exclude`, `-c/--category` (comma-separated), `-n/--limit` (20), `--per-source` (10, at most 100), `--sort relevance|newest`, `--refresh`, `--offline`, `--timeout SECS` (20; 0 waits for every source). Their meaning is the MCP argument table in `references/search.md`.

A flag beats its variable, which beats the default:

| Variable | Sets |
|---|---|
| `DATASEEK_LIMIT`, `DATASEEK_PER_SOURCE`, `DATASEEK_TIMEOUT`, `DATASEEK_EXCLUDE` | the `search` flags of the same name |
| `DATASEEK_CACHE_DIR`, `DATASEEK_CONNECT_TIMEOUT` | the global flags of the same name |
| `DATASEEK_CACHE_MAX_MB` | the cache budget, 1 to 10000, 30 by default |
| `NO_COLOR`, `CLICOLOR_FORCE`, `COLUMNS` | color and width of text output |
| `ALL_PROXY`, `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` | the proxy |
| `SSL_CERT_FILE`, `SSL_CERT_DIR` | Linux root certificates |

A script that must not inherit a user's `DATASEEK_TIMEOUT=0` or `DATASEEK_EXCLUDE` passes the flag explicitly. Key variables are in `references/setup.md`, Keys.

## Streams and schemas

- **stdout is data.** With `--json`, one object, indented on a terminal and one line in a pipe, tagged with `schema`: `dataseek-search/1`, `dataseek-sources/1`, `dataseek-inspect/1`, `dataseek-doctor/1`, `dataseek-cache-info/1`, `dataseek-cache-warm/1`, `dataseek-surface/1` (from `help --json`). A new key may appear without notice; a changed or removed one bumps the number, so a script can assert the tag.
- **stderr is status.** With `--json`, each line is an event `{"schema": "dataseek-events/1", "event": ..., "message": ...}`: `stage` (what it is doing), `warning` (a source that failed, a stale catalog), `ok` (the summary, including the total match count), and `error` (with `causes` and `try`). `-q` leaves only the error.
- **A failed run prints nothing on stdout.** `jq` exits 0 on empty input, so run pipelines under `set -o pipefail` in bash and zsh, or check `$pipestatus[1]` in fish.
- **An empty search is success** (exit 0, `results: []`) when any source answered. Exit codes: `references/troubleshoot.md`.

## Recipes

```sh
set -o pipefail

# one URL per line
dsk search census --json | jq -r '.results[].url'

# title, URL and holding sources as TSV
dsk search census --json | jq -r '.results[] | [.title, .url, (.sources | join(","))] | @tsv'

# only datasets more than one archive holds
dsk search sea surface temperature --json | jq '[.results[] | select(.sources | length > 1)]'

# only Creative Commons licenses, as the sources write them (URL or name)
dsk search sea surface temperature --json \
  | jq '[.results[] | select((.license // "") | test("creativecommons|cc-?by|cc0"; "i"))]'

# which sources did not answer, and why
dsk search sea surface temperature --json | jq -r '.sources[] | select(.answered | not) | "\(.id)\t\(.status)"'

# fail the script when the search did not wait for every source
dsk search sea surface temperature --json | jq -e '.complete' > /dev/null

# keep the events: warnings and the summary line with the total
dsk search ERSST --json 2> events.ndjson > results.json
jq -r 'select(.event == "warning" or .event == "ok") | .message' events.ndjson

# every file link a dataset page declares
dsk inspect https://zenodo.org/records/13135140 --json | jq -r '.files[].url'
```

## Batches

Run queries one after another; each search already fans out to every source, and the cache answers a repeat within 6 hours. Merge by URL afterwards:

```sh
set -o pipefail
for q in "sea surface temperature" "ERSST"; do
  dsk search "$q" -n 10 --json -q | jq -c --arg q "$q" '.results[] | {query: $q, title, url, sources}'
done | jq -s 'unique_by(.url)'
```

For a batch larger than a few dozen queries, set `HF_TOKEN` and `GITHUB_TOKEN` first: Hugging Face allows 500 requests per 5 minutes per address without a key, GitHub 10 searches a minute.

## Discovering the surface

`dsk help --json` (schema `dataseek-surface/1`) lists every command, flag (with `type`, `values`, `default`, `minimum`, `maximum`, `env`), variable and exit code, generated from the parser itself. `dsk help search --json` prints one command's entry. When this reference and that output disagree, the output is right.
