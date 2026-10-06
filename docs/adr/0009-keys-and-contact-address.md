# ADR 0009: Keys come from the environment or files, never flags

- Status: accepted
- Date: 2026-10-06

## Context

Eight sources take an API key (Hugging Face, Kaggle, data.gov, GitHub, FRED, Roboflow, Data Commons, NCBI). The contract forbids secrets on the command line (`../reference/contract.md`). Polite-pool APIs (DataCite, NCBI) want a contact address, and that address ends up in their logs.

## Decision

- A key is read from its environment variable, then from `credentials.toml` in the config directory. Kaggle additionally reads what its own CLI writes, `~/.kaggle/access_token` then the legacy `~/.kaggle/kaggle.json` (honoring `KAGGLE_CONFIG_DIR`), so a user who ran `kaggle auth` needs no setup. Without a token Kaggle is still asked, anonymously, for its first page of 20.
- Secrets are never formatted into output; `sources` and `doctor` report only where a key came from. `doctor` fails a key file other users can read.
- Every request's User-Agent carries the project contact `user@dataseek.dev`, and DataCite and NCBI also receive it as `mailto`/`email`. It is the project's address, the same for every user; no user's address is ever sent.

## Consequences

- Sources with an optional key work out of the box; FRED, Roboflow and Data Commons are skipped until their key is set, and say so. Data Commons publishes a trial key, but its documentation allows it for single requests and asks anyone building an application for an own key, so dataseek never ships it.
- Rotating a key never touches dataseek's config beyond the one line or variable.

## Evidence

- DataCite gives identified clients 1,000 requests per 5 minutes against 500 anonymous; api.data.gov gives a personal key 1,000 requests an hour against 30 for `DEMO_KEY` (both documented, checked 2026-10-06).
