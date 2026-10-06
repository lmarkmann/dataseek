# Keys and cache

## API keys

No key is needed for most sources: 73 of the 76 answer without one. Keys raise rate limits or unlock three sources. Set a key as an environment variable, or put it in `credentials.toml` in the config directory (`dataseek doctor` prints the path) with permissions `600`:

```toml
fred = "..."
datagov = "..."
```

| source | environment variable | file key | without it | get one |
|---|---|---|---|---|
| Hugging Face | `HF_TOKEN` | `huggingface` | 500 requests per 5 min per IP | https://huggingface.co/settings/tokens |
| Kaggle | `KAGGLE_API_TOKEN`, or `~/.kaggle/access_token`, or `~/.kaggle/kaggle.json` | `kaggle` | anonymous first page of 20 | https://www.kaggle.com/settings/api |
| Data.gov | `DATAGOV_API_KEY` | `datagov` | catalog.data.gov's keyless search | https://api.data.gov/signup/ |
| GitHub | `GITHUB_TOKEN` | `github` | 10 searches per minute | https://github.com/settings/tokens |
| FRED | `FRED_API_KEY` | `fred` | source skipped | https://fredaccount.stlouisfed.org/apikeys |
| Roboflow Universe | `ROBOFLOW_API_KEY` | `roboflow` | source skipped | https://app.roboflow.com/settings/api |
| Data Commons | `DATACOMMONS_API_KEY` | `datacommons` | source skipped | https://apikeys.datacommons.org |
| NCBI | `NCBI_API_KEY` | `ncbi` | 3 requests per second | https://account.ncbi.nlm.nih.gov/settings/ |

An environment variable is visible to every program started from that shell and to anything that dumps the environment (a crash report, `ps e`, a CI log); it suits CI and one-off runs. For a key that stays on a machine, prefer `credentials.toml` with permissions `600`.

Kaggle's files are the ones its CLI writes (`kaggle auth login`, or the token from the settings page); `KAGGLE_CONFIG_DIR` moves them. `dataseek sources` shows each key's status and `dataseek doctor` where it was found; neither ever prints a key, and `doctor` fails a key file other users can read.

A data.gov key: sign up at https://api.data.gov/signup/ with a name and an address, and the key arrives by email at once. It gives 1,000 requests an hour; the shared `DEMO_KEY` gives 30 an hour and 50 a day.

Every request carries the contact address `user@dataseek.dev` in its User-Agent (and as `mailto` for DataCite, `email` for NCBI). It is the project's address and the same for every user.

## Cache

| kind | lives | what |
|---|---|---|
| query results | 6 hours | one file per source and query; never for Kaggle |
| catalogs | 7 days | the full lists of the `local` sources |
| outage marks | 10 minutes | sources skipped after an outage unless named with `--source` |

The cache directory is trimmed to 30 MB and 2,000 files after every search, oldest first. `--cache-dir DIR` or `DATASEEK_CACHE_DIR` moves it.

```sh
dataseek cache info                 # path, size and budget
dataseek cache warm                 # download every catalog now, no deadline
dataseek cache clear --dry-run      # what clearing would delete
dataseek cache clear                # delete every entry; other files in the directory stay
dataseek search ... --refresh       # ask every live source again
dataseek search ... --offline       # cached answers and catalogs only, no network
dataseek search ... --timeout 0     # wait for every source, however slow
```

A fetch that fails serves the expired entry when one exists; `-v` and `--json` label a query answered that way as stale, while a catalog searched from an expired copy still reads `ok`. An entry written by another release counts as expired, because that release may have parsed the source differently: it is refetched when online and still served when the fetch fails. `--offline` sends no request at all, so it never marks a source as down. Why the numbers are what they are: [ADR 0007](../adr/0007-cache-budget-and-failure-handling.md).
