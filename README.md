# dataseek

<!-- repo-description -->
Search for datasets from the terminal
<!-- /repo-description -->

One query, 76 dataset sources, one deduplicated list: machine-learning hubs (Hugging Face, Kaggle, OpenML), research repositories and DOI registries (DataCite, Zenodo, Dataverse), government portals (data.europa.eu, Data.gov, CKAN and Socrata portals), economics and finance (DBnomics, IMF, OECD, ECB, BIS, Bundesbank, FRED), earth observation (NASA CMR, STAC catalogs, Copernicus), life sciences, neuroscience, physics and social science archives, and Google Dataset Search. `dsk` is the same program under a shorter name.

## Install

```sh
cargo install --path .   # installs dataseek and dsk
```

## Usage

```sh
dsk search sea surface temperature          # every source, merged and ranked
dsk search mnist -s huggingface,kaggle      # only these sources
dsk search inflation -c economics,finance   # only these categories
dsk search census --json | jq -r '.results[].url'
dsk sources                                 # every source, its protocol, key and docs
dsk inspect https://zenodo.org/records/1234567   # a page's schema.org or Croissant metadata
dsk bench                                   # latency and overlap of every source
dsk cache warm                              # download the catalogs searched locally
```

`dsk` alone shows the commands, `dsk help environment` the variables it reads, `dsk help exit-codes` what it returns.

No key is needed; keys for Hugging Face, Kaggle, Data.gov, GitHub, FRED, Roboflow, Data Commons and NCBI raise limits or unlock a source. See [`docs/reference/keys-and-cache.md`](docs/reference/keys-and-cache.md).

## Development

```sh
just check   # fmt --check + clippy -D warnings + tests
just ci      # everything the CI job runs: check, startup bench, typos, Windows and macOS cross-check, shear, msrv, audit
just audit   # cargo-deny + zizmor
just run search climate temperature
```

## Docs

- [`docs/CHANGELOG.md`](docs/CHANGELOG.md): what shipped, written by release-plz.
- [`docs/reference/`](docs/reference/): setup, development, the CLI contract, keys and cache, security, and the release loop.
- [`docs/adr/`](docs/adr/): why it is built this way, including [the full source list](docs/adr/0004-every-source-is-searched.md), plus what was [rejected](docs/adr/rejected.md) and what is [parked](docs/adr/watchlist.md). Read both before proposing an addition.

### Project notes

- [`docs/bench/`](docs/bench/): measured speed of the search stages and of startup, each with the machine it ran on.
- [`docs/synthesis/`](docs/synthesis/): how the sources' search interfaces differ, and the bench that measures them.
