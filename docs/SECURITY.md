# Security policy

## Reporting a vulnerability

Report it privately through [GitHub's advisory form](https://github.com/lmarkmann/dataseek/security/advisories/new). If you cannot use GitHub, write to contact@dataseek.dev. Please do not open a public issue for it.

A useful report names the version (`dsk --version`), the platform, the command you ran, and what happened. When remote content triggers it, add the source id and the response body that does it; a saved file is enough.

You will hear back within a week. The fix ships as a patch release, and the advisory is published with it, crediting you unless you ask not to be named.

## Scope

In scope:

- The `dataseek` and `dsk` binaries, including the MCP server that `dsk mcp` runs.
- Keys: a key from the environment, `credentials.toml` or Kaggle's token files reaching a URL, an error message, a log line, the cache, or any host other than the source it belongs to ([ADR 0009](adr/0009-keys-and-contact-address.md)).
- The cache directory: remote text reaching a file path, or a cache file another local user can plant or read.
- Remote text driving the terminal: control characters or escape sequences from a source's response, a page `inspect` reads, or a source's own error message reaching stdout or stderr.
- The release pipeline: the workflows, the tags, and the binaries and wheels they publish.

Out of scope:

- The sources themselves. A source serving wrong, stale or misleading data, or a flaw in its own API, belongs with that provider.
- A vulnerability in a dependency that dataseek never reaches. Report it upstream; `cargo-deny` fails CI on any published RUSTSEC advisory ([`reference/security.md`](reference/security.md)).
- Attacks that start from control of your environment variables or your config directory.
