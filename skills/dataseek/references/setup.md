# Setup

Verified against `README.md`, `docs/reference/contract.md`, `docs/reference/keys-and-cache.md` and a live `dsk mcp` session on 2026-10-20.

## Contents

- **Install**: which command, and how to tell it worked
- **Register the MCP server**: the entry every client takes, and the three variants that fix a server that will not start
- **Verify**: the one call that proves the tools work, and the probe for when they do not appear
- **Keys**: which sources a key unlocks or speeds up, and where a key may live
- **Cache**: when to warm the catalogs and where the cache sits

## Install

Check first: `dsk -V` prints `dataseek <version>`. Otherwise, any one of:

```sh
brew install lmarkmann/tap/dataseek      # macOS and Linux with Homebrew
cargo binstall dataseek                  # prebuilt binary through cargo
cargo install --locked dataseek          # build from source
uv tool install dataseek                 # from PyPI
uvx dataseek search sea surface temperature   # run once, install nothing
```

Every method installs two names for one program, `dsk` and `dataseek`. Installing changes the user's machine: ask before running one, and let the user pick the package manager they already use.

## Register the MCP server

The entry, under whatever key the client keeps its servers (`mcpServers` in most JSON configs):

```json
{
  "mcpServers": {
    "dataseek": { "command": "dsk", "args": ["mcp"] }
  }
}
```

Many clients also have a command that writes the entry, typically `<client> mcp add dataseek -- dsk mcp`; check the client's own help for its form before guessing one.

Variants, each for one failure:

- **The client cannot find `dsk`.** A desktop app often starts without the shell's `PATH`. Put the absolute path from `command -v dsk` in `command`.
- **No install, only `uv`.** `"command": "uvx", "args": ["dataseek", "mcp"]`.
- **A cache directory or connect timeout.** Global flags go after `mcp` and reach every call: `"args": ["mcp", "--cache-dir", "/path/to/cache", "--connect-timeout", "5"]`. `-q` and `-v` work the same way.
- **An extra source on every search.** `"args": ["mcp", "--include", "google"]` adds Google Dataset Search to every search that sets neither `source` nor `include`. Only on the user's request: Google and Mendeley are opt-in because neither permits automated searches.

The server speaks JSON-RPC on stdin and stdout and writes its own status line, `serving search, sources, inspect over MCP on stdio`, plus each call's narration on stderr, which clients usually show in their MCP logs. It serves protocol revisions 2025-06-18 and 2025-11-25 through `initialize`, and 2026-07-28 statelessly.

After upgrading dataseek, restart the server in the client: a running server keeps the binary and the tool list it started with.

## Verify

Call the `sources` tool once with no arguments. It answers with 76 rows; count them and note any `key` of `missing`. That count is the Setup lane's **Verified** part.

When the tools do not appear at all, probe the server from a shell. This sends the handshake and a tool list request, holding stdin open for three seconds because closing it ends the server and every call still running:

```sh
( printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'; sleep 3 ) | dsk mcp
```

Two JSON lines back, the second naming `search`, `sources` and `inspect`, means the server works and the fault is in the client's entry: the command path, the JSON, or a client that needs restarting. No output, or `command not found`, is the install.

## Keys

73 of the 76 sources need no key. A key unlocks three, raises the limits of four, and Kaggle reads one but answers the same without:

| Source | Variable | Without it |
|---|---|---|
| FRED | `FRED_API_KEY` | skipped (`needs $FRED_API_KEY`) |
| Roboflow Universe | `ROBOFLOW_API_KEY` | skipped |
| Data Commons | `DATACOMMONS_API_KEY` | skipped |
| Hugging Face | `HF_TOKEN` | 500 requests per 5 minutes per IP |
| GitHub | `GITHUB_TOKEN` | 10 searches per minute |
| NCBI (GEO) | `NCBI_API_KEY` | 3 requests per second |
| Data.gov | `DATAGOV_API_KEY` | the keyless catalog search |
| Kaggle | `KAGGLE_API_TOKEN`, or the Kaggle CLI's `~/.kaggle/access_token` or `~/.kaggle/kaggle.json` | anonymous requests work |

Each source's `key` in the `sources` result reads `none` (takes no key), `optional` (takes one, none set, works without), `set` (found) or `missing` (needs one, none set, skipped). `dsk doctor` says where each key was found and never prints one.

Where a key lives, in order of preference:

1. **`credentials.toml`** in dataseek's config directory, which `dsk doctor` prints, with mode 600; `doctor` fails a file other users can read. One line per key, by source id: `fred = "..."`, `datagov = "..."`, `huggingface = "..."`. The user writes the value; never echo it into a command.
2. **The environment the server starts in.** A key exported in the user's shell profile reaches `dsk` in a terminal, but a desktop client may not inherit it; then use the file.
3. **The client's `env` block** for the server entry, only in a config file that lives outside any repository and that the user chose. A key in a project's committed MCP config is a leaked key.

No tool and no flag takes a key, so a key never belongs in a tool argument or on a command line.

## Cache

Query answers are cached 6 hours, catalogs (the `*` sources in `references/search.md`) 7 days, and a failed source rests 10 minutes. A first search, or a search after a week, reports catalogs as `expired catalog` or `downloading its catalog`; one warm-up fixes both:

```sh
dsk cache warm                  # download every catalog now, no deadline
dsk cache warm -s openneuro     # only these
dsk cache info                  # path, size and budget
dsk cache clear --dry-run       # what clearing would delete
```

`cache warm` and `cache clear` are CLI only; no MCP tool runs them. `cache clear` deletes every cached answer and catalog: show the `--dry-run` output and ask before running it.
