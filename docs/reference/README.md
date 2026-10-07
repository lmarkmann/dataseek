# Reference

How the project works and how to operate it, one file per domain. Each file describes the current state; when that state changes, the file changes with it.

What is not here: why a choice was made ([`../adr/`](../adr/)) and what shipped ([`../CHANGELOG.md`](../CHANGELOG.md)).

| file | covers | read when |
|---|---|---|
| [setup.md](setup.md) | dev tools to install, `just description` | setting up a machine |
| [development.md](development.md) | daily recipes, lints, tests, dependencies, MSRV | writing code |
| [contract.md](contract.md) | flags, streams, errors, exit codes, color, the lints that enforce them | changing what the CLI prints or accepts |
| [security.md](security.md) | cargo-deny, zizmor, action pins, workflow permissions | touching dependencies or workflows |
| [keys-and-cache.md](keys-and-cache.md) | API keys, where they are read from, the contact address, the cache and its commands | setting up a key or wondering why a result is old |
| [release.md](release.md) | conventional commits, the release loop, `.github/release-plz.toml`, the App token | merging anything, or when a release did not happen |
