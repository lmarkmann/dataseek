![dataseek: every dataset source it searches, set out as a periodic table](https://raw.githubusercontent.com/lmarkmann/dataseek/main/docs/assets/dataseek-banner.png)

[![crates.io](https://img.shields.io/crates/v/dataseek?style=flat-square&label=crates.io&color=dea584&logo=rust&logoColor=black)](https://crates.io/crates/dataseek)[![PyPI](https://img.shields.io/pypi/v/dataseek?style=flat-square&label=PyPI&color=3775A9&logo=pypi&logoColor=white)](https://pypi.org/project/dataseek/)[![Homebrew](https://img.shields.io/badge/brew-lmarkmann%2Ftap-FBB040?style=flat-square&logo=homebrew&logoColor=black)](https://github.com/lmarkmann/dataseek)[![License](https://img.shields.io/crates/l/dataseek?style=flat-square&color=2DA44E)](https://github.com/lmarkmann/dataseek)

<!-- repo-description -->
Search for datasets from the terminal
<!-- /repo-description -->

Datasets sit in dozens of archives: Hugging Face and Kaggle, Zenodo and Dataverse, data.gov and data.europa.eu, the IMF and the ECB, NASA and Copernicus. Each has its own search box and its own idea of relevance, and many host the same dataset under different names. dataseek queries them all at once and returns one deduplicated, ranked list, naming every archive that holds each result. Datasets carried by more archives, and matching more of your query, rank higher.

Most sources need no key and nothing to set up. Results come back in the terminal, as JSON for scripts, or to an AI client over MCP. Google Dataset Search and Mendeley Data are the exception: neither permits automated searches, so dataseek asks them only when you name them with `-s` ([ADR 0013](docs/adr/0013-opt-in-sources.md)). `dsk` is the same program under a shorter name.

## Install
```sh
brew install lmarkmann/tap/dataseek
cargo binstall dataseek              # prebuilt binaries, otherwise this also works:

cargo install --locked dataseek
uv tool install dataseek

# To run it once without installing:
crgx --allow-build dataseek search sea surface temperature
uvx dataseek search sea surface temperature
```

## Usage
```sh
dsk search sea surface temperature                 # every default source, merged and ranked
dsk search mnist -s huggingface,kaggle             # only these sources
dsk search inflation -c economics,finance          # only these categories
dsk search census --json | jq -r '.results[].url'  # one URL per line, for scripts
dsk sources                                        # every source, its protocol, key and docs
dsk inspect https://zenodo.org/records/13135140    # a page's metadata and the files it lists
```

`dsk search air quality -n 3` on a terminal:

```text
- searching 71 sources for "air quality"; 3 need a key, see `dsk doctor`
1  Improving Indoor Air Quality for Poor Families: Controlled Experiments 2005-2006
   https://microdata.worldbank.org/catalog/396
   worldbank-microdata, ihsn | Susmita Dasgupta, Mainul Huq, M. Khaliquzzaman and David Wheeler | 2...
   Indoor air pollution (IAP) is dangerously high for many poor families in Bangla...

2  Air quality data in five counties in California
   https://b2find.eudat.eu/dataset/f2611e4d-793c-5e55-950c-7a9979c772b8
   b2find, pangaea | PANGAEA | 2026-05-04 | doi:10.1594/pangaea.923005
   Air quality data used to identify time windows of the best air quality in the d...

3  Historical Air Quality
   https://data.calgary.ca/d/uqjm-jxgp
   openaire, socrata | City of Calgary Open Data Portal | 2018-07-16 | See Terms of Use
   This dataset makes historical air quality data accessible for various parameter...

  ✓ 3 of 359 results; 70 of 71 sources answered; -n 359 shows all
```

`dsk` alone shows the commands, `dsk help environment` the variables it reads, `dsk help exit-codes` what it returns. `cargo install` cannot install a man page or completions: `dsk man > ~/.local/share/man/man1/dsk.1` puts the page where `man dsk` finds it, and `dsk completion fish` (or `bash`, `zsh`) says where its script goes.

While, most of the sources don't require an API-key FRED, Roboflow and Data Commons are skipped without one; keys for Hugging Face, Kaggle, Data.gov, GitHub and NCBI raise download limits. See: [`docs/reference/keys-and-cache.md`](docs/reference/keys-and-cache.md).

## MCP
Use it in the terminal, pipe its JSON into scripts, or use dataseek via MCP (`dsk mcp`), so Claude, Cursor or any other MCP client can search and inspect datasets for you. Most sources need no API key. Google Dataset Search and Mendeley Data don't permit automated searches, so dataseek only queries them when named with `-s` ([ADR 0013](docs/adr/0013-opt-in-sources.md)). Register it in the client's MCP server configuration:

```json
{
  "mcpServers": {
    "dataseek": { "command": "dsk", "args": ["mcp"] }
  }
}
```

Each tool returns what its command prints with `--json`; keys are read from the environment the client starts it in, or from `credentials.toml`. Google Dataset Search and Mendeley Data stay opt-in here too; the model asks for them by naming them in the search tool's `source` argument. See: [`docs/reference/contract.md`](docs/reference/contract.md#mcp-mode).

## Contributing & Development Docs
- [`CONTRIBUTING.md`](CONTRIBUTING.md) covers setup, the commit prefixes that decide each release, the test rules and how to add a source. Report vulnerabilities privately, as [`SECURITY.md`](docs/SECURITY.md) describes.
- [`docs/CHANGELOG.md`](docs/CHANGELOG.md): what shipped, written by release-plz.
- [`docs/reference/`](docs/reference/): setup, development, the CLI contract, keys and cache, the supply-chain gates, and the release loop.
- [`docs/adr/`](docs/adr/): why it is built this way, including [the full source list](docs/adr/0004-every-source-is-searched.md), plus what is [parked](docs/adr/watchlist.md). Read it before proposing an addition.
- [`docs/bench/`](docs/bench/): measured speed of the search stages and of startup, each with the machine it ran on.
- [`docs/synthesis/`](docs/synthesis/): how the sources' search interfaces differ, and the bench that measures them.

```sh
just check                            # fmt --check, clippy -D warnings, tests, doctests
just ci                               # everything CI gates on: check, relevance, startup bench, Windows and macOS cross-check, shear, msrv, audit, typos
just audit                            # cargo-deny, zizmor
just run search climate temperature   # the CLI built from this checkout
```

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT); at your choice.
