# Sources

Verified against the `sources` tool (106 rows), `docs/synthesis/search-methods.md`, `docs/synthesis/terms.md` and live searches on 2026-11-23. The "holds" columns are orientation written from each archive's own description; the archive's page decides what it holds.

Open this to choose which sources a search should ask, to know what one source's records are, and to explain why a source returned what it did.

## Contents

- **The live list**: the fields of a `sources` row and what each value means
- **Choose sources for a need**: the user's need, and the `category`, `source` or `include` that fits it
- **What each source holds**: all 106, by category
- **What one record is**: sources whose "dataset" is a series, a table, a collection or a repository
- **How sources match a query**: which ones need every word, which match names only
- **Local catalogs**: the 39 sources searched on this machine, and their sizes
- **Slow, limited and keyed sources**: the ones a default search often misses, and what to do
- **Opt-in sources and notices**: what must be said to the user
- **The answer**: the Sources and Checked parts, filled in

## The live list

Call `sources` (no arguments) for the list the running server has. It returns `{"schema": "dataseek-sources/1", "sources": [...]}`, one row per source:

| Field | Values | Use |
|---|---|---|
| `id` | `zenodo`, `data-gov-uk`, ... | what `source`, `include` and `exclude` take |
| `name` | `Zenodo`, `data.gov.uk`, ... | the name to show a user |
| `category` | one of 14 | what `category` takes |
| `protocol` | `CKAN`, `Dataverse`, `SDMX`, `STAC`, `Hub API`, ... | sources with one protocol match and page the same way |
| `search` | `live` or `local` | `live`: the query goes to the source. `local`: dataseek searches a downloaded copy of the source's whole list |
| `key` | `none`, `optional`, `set`, `missing` | `missing`: the source is skipped until its key is set |
| `key_env` | a variable name, or null | the variable that holds the key |
| `opt_in` | a reason, or null | not null: asked only when a call names it |
| `notice` | a sentence, or null | not null: the provider's terms ask that the user sees it |
| `docs` | a URL | the provider's API documentation |

As of 2026-11-23: 106 sources, 67 `live` and 39 `local`. 98 take no key, 5 take an optional one, 3 need one. A default search asks 104; `google` and `mendeley` are opt-in.

The ids in this file are for reading. Before you put an id in a call you have not used in this task, take it from the tool's schema or the `sources` result.

## Choose sources for a need

Leave the selection alone unless the user's words or the topic call for one. The default set is the right start for a topic you cannot place.

| The user needs | Set | Why |
|---|---|---|
| a topic, no archive named | nothing | every default source is asked |
| machine-learning training or benchmark data | `category: ["machine-learning"]`; add `"code"` for repositories | hubs, benchmark lists and corpora |
| the data behind a paper, or anything with a DOI | `category: ["aggregator", "research"]` | DataCite and OpenAIRE index most repositories; the repositories add files and licenses |
| a dataset on one named archive | `source: ["<id>"]` | that archive alone, with the whole `timeout` |
| "also check <archive>" | `include: ["<id>"]` | that archive beside the default set |
| official statistics, indicators | `category: ["statistics", "economics"]` | national and international statistical offices |
| economic or financial time series | `category: ["economics", "finance"]` | `dbnomics` aggregates most providers' series; `fred` needs its key |
| household or survey microdata | `source: ["worldbank-microdata", "ihsn", "fao-microdata", "unhcr-microdata", "cessda"]` | the survey catalogs |
| a government's open data | `category: ["government"]`, or the country's id | national portals and portal networks |
| satellite or earth-observation products | `category: ["geospatial"]` | collections, not single scenes |
| climate, ocean, earth-science measurements | `source: ["pangaea", "ncei", "cmr", "copernicus-cds", "dataone"]` or `category: ["geospatial", "ecology"]` | the earth-science archives |
| species occurrence, ecology | `category: ["ecology"]` | `gbif`, `dataone` |
| genomics, omics, single cell | `category: ["life-sciences"]`; name the EBI ids for them (below) | sequence, expression and omics archives |
| medical imaging, clinical signals | `source: ["tcia", "grandchallenge", "physionet", "openneuro"]` | imaging collections, challenges, waveforms |
| microscopy, cryo-EM raw data | `source: ["bioimages", "empiar"]`, `timeout: 150` | both are EBI Search domains and slow |
| speech, text and translation corpora | `source: ["huggingface", "mozilla", "openslr", "clarin", "elg", "opus"]` | language-resource catalogs |
| social-science studies | `category: ["social-science"]` | European and national study catalogs |
| particle physics, materials | `category: ["physics"]` | `cern`, `materials-project`, `nomad` |
| datasets several archives agree on | `min-sources: 2` | a filter, not a selection |

`category` is coarse. `datacite`, `openaire` and `b2find` sit in `aggregator`, so `category: ["life-sciences"]` leaves them out. When recall matters more than focus, run a second search without `category`.

A wrong id is refused with the list of valid ones. Common mappings: "Hugging Face" is `huggingface`, "data.gov" is `datagov`, "the World Bank" is `worldbank` (indicators) plus `worldbank-microdata` (surveys), "NASA Earthdata" is `cmr`, "Earth Engine" is `earth-engine`, "the EU portal" is `europa`, "Dataverse" is `harvard-dataverse` plus the nine other installations.

## What each source holds

`*` marks a `local` source. `(key)` needs a key; `(opt-in)` is asked only when named.

**`aggregator`**: indexes of other archives.

| Id | Holds |
|---|---|
| `datacite` | DOI records of the dataset type from every DataCite member repository |
| `openaire` | research products of the dataset type, harvested from European and other repositories |
| `b2find` | research data from the EUDAT network's European repositories |
| `google` (opt-in) | Google Dataset Search: pages that publish dataset markup |

**`machine-learning`**: training data, benchmarks, corpora.

| Id | Holds |
|---|---|
| `huggingface` | dataset repositories on the Hugging Face Hub |
| `kaggle` | Kaggle datasets |
| `openml`* | tabular datasets with benchmark tasks |
| `uci`* | the UCI Machine Learning Repository |
| `roboflow` (key) | computer-vision datasets on Roboflow Universe |
| `modelscope` | datasets on ModelScope, many with Chinese metadata |
| `mozilla` | the Mozilla Data Collective: speech and language datasets, the free ones |
| `clarin` | language corpora in the CLARIN Virtual Language Observatory |
| `elg` | language resources in the European Language Grid |
| `opus`* | parallel corpora for translation; the list holds names only |
| `openslr`* | speech and language resources |
| `aws`* | the Registry of Open Data on AWS |
| `tfds`* | the TensorFlow Datasets catalog; names and category headings only |
| `lila`* | LILA BC: labeled images for conservation and ecology |
| `polaris`* | Polaris Hub: drug-discovery benchmark datasets |
| `academictorrents`* | large datasets distributed as torrents |
| `googleresearch`* | datasets Google Research has released |
| `neurips`* | papers of the NeurIPS Datasets and Benchmarks track, 2022 to 2026 |
| `pwc`* | the Papers with Code dataset index, frozen on 2025-07-28 |

**`code`**: repositories.

| Id | Holds |
|---|---|
| `github` | GitHub repositories with the topic `dataset` |
| `gitlab` | GitLab projects with the topic `dataset` |

**`research`**: general research-data repositories.

| Id | Holds |
|---|---|
| `zenodo` | Zenodo records, any discipline |
| `figshare` | Figshare items, any discipline |
| `harvard-dataverse` | Harvard Dataverse, any discipline, strongest in social science |
| `borealis` | the Canadian universities' Dataverse |
| `recherche-data-gouv` | the French national research-data repository |
| `dataverse-nl`, `dataverse-no` | the Dutch and Norwegian Dataverse installations |
| `darus`, `juelich` | University of Stuttgart and Forschungszentrum Juelich |
| `cora` | CORA.RDR, the Catalan research-data repository |
| `osf` | OSF projects and registrations, through the SHARE index |
| `mendeley` (opt-in) | Mendeley Data |

**`government`**: open-data portals.

| Id | Holds |
|---|---|
| `europa` | data.europa.eu: the EU's harvest of member-state portals, multilingual |
| `datagov` | the United States federal catalog |
| `data-gov-uk`, `open-canada`, `data-gov-au`, `govdata`, `data-go-jp`, `opendata-swiss` | the national portals of the United Kingdom, Canada, Australia, Germany, Japan and Switzerland |
| `hdx` | the Humanitarian Data Exchange: crisis and development data by country |
| `energydata` | the World Bank's energy-sector data |
| `open-africa` | openAFRICA: African open data from many publishers |
| `netl-edx` | the US Department of Energy NETL Energy Data eXchange |
| `socrata`, `socrata-eu` | city, county and state portals that run on Socrata, US and EU hosts |
| `opendatasoft` | portals that run on OpenDataSoft |
| `arcgis` | ArcGIS Hub: datasets and map layers from local governments |

**`statistics`**, **`economics`**, **`finance`**: indicators, series and survey catalogs.

| Id | Holds |
|---|---|
| `eurostat`* | Eurostat tables |
| `undata`*, `imf`*, `oecd`*, `ilo`*, `ecb`*, `bis`*, `bundesbank`*, `unicef`* (in `social-science`) | each agency's SDMX dataflows, which are statistical tables |
| `worldbank`* | World Bank indicators |
| `who`* | WHO Global Health Observatory indicators |
| `census`* | the datasets of the US Census Bureau API |
| `fiscal-data`* | US Treasury Fiscal Data tables |
| `dbnomics` | datasets of series from statistical offices and central banks, in one index |
| `owid` | Our World in Data charts and explorer views |
| `datacommons` (key) | Data Commons statistical variables |
| `fred` (key) | FRED economic series |
| `worldbank-microdata`, `ihsn`, `fao-microdata`, `unhcr-microdata` | survey and census microdata catalogs |

**`geospatial`** and **`ecology`**: earth observation and environment.

| Id | Holds |
|---|---|
| `cmr` | NASA Earthdata collections |
| `ncei` | NOAA NCEI climate, ocean and weather archives |
| `pangaea` | PANGAEA: earth and environmental science datasets |
| `copernicus-cds`* | Copernicus Climate Data Store datasets |
| `copernicus-dataspace`*, `planetary-computer`*, `earth-search`*, `usgs-landsat`*, `swisstopo`* | STAC collections: Sentinel, Landsat and other imagery products, Swiss geodata |
| `earth-engine`* | the Google Earth Engine catalog |
| `gbif` | biodiversity datasets registered with GBIF |
| `dataone` | ecology and environment datasets from the DataONE member repositories |

**`life-sciences`** and **`neuroscience`**.

| Id | Holds |
|---|---|
| `geo` | NCBI GEO gene-expression records |
| `arrayexpress`, `biostudies` | functional-genomics experiments and study records at EMBL-EBI |
| `bioimages`, `empiar` | the BioImage Archive, and raw electron-microscopy images |
| `omicsdi` | omics datasets across many repositories |
| `encode` | ENCODE experiments |
| `cellxgene`* | CZ CELLxGENE single-cell collections |
| `synapse` | Synapse: biomedical research projects |
| `tcia`* | The Cancer Imaging Archive collections |
| `grandchallenge`* | medical image analysis challenges |
| `openneuro`* | neuroimaging datasets |
| `dandi` | DANDI: neurophysiology datasets |
| `physionet`* | physiological signals and clinical datasets |

**`physics`** and **`social-science`**.

| Id | Holds |
|---|---|
| `cern` | CERN Open Data |
| `materials-project`* | datasets contributed to the Materials Project |
| `nomad`* | NOMAD materials-science datasets |
| `cessda` | studies in the European social-science data archives |
| `aussda` | the Austrian Social Science Data Archive |
| `qdr` | the Qualitative Data Repository |

## What one record is

A result is whatever the source lists. Tell the user when that is not a file-bearing dataset:

| Sources | One record is |
|---|---|
| `fred` | one time series |
| `worldbank`, `who`, `datacommons` | one indicator or statistical variable |
| `owid` | one chart or explorer view |
| the SDMX sources, `eurostat` | one table (a dataflow), often one per country, year or version with one title |
| `dbnomics` | one provider's dataset of series |
| the STAC sources, `cmr`, `earth-engine` | one collection or product, not a scene or a granule |
| `encode` | one experiment |
| `neurips` | one paper of the track; the data is linked from the paper |
| `pwc` | one index entry with the dataset's homepage |
| `github`, `gitlab` | one repository; its files are in the repository, and `related` lists the owner's others |
| `osf` | one project or registration |
| `grandchallenge` | one challenge; the data sits behind its page |

Several results with one title from one source are different records. Tell them apart by URL and `description`.

## How sources match a query

dataseek sends the query as typed to every source, and each source runs its own engine. A word added is a filter at most of them.

| Matching | Sources |
|---|---|
| every word must match | the 39 `local` catalogs (each word as the start of a token), `pangaea`, `cessda`, `modelscope`, `encode`, `dandi`, `gitlab`; `socrata` and `socrata-eu` for up to three terms |
| any word, ranked | the four survey catalogs (`worldbank-microdata`, `ihsn`, `fao-microdata`, `unhcr-microdata`), `ncei`, `synapse`, `elg`, `mozilla` |
| names and short fields only | `huggingface` for one word (the repository id; several words also match the dataset card), `kaggle` (title, subtitle, tags), `dbnomics` (names and codes), `roboflow` (names, classes), `opus` and `tfds` (names) |
| full text, relevance ranked | `datacite`, `openaire`, `zenodo`, the Dataverse and CKAN sources, `europa`, `datagov`, `gbif`, `dataone`, `cmr`, `cern`, `clarin`, `osf` |
| no relevance order | `figshare` (newest first), `huggingface` (most downloaded first), `gitlab` (most starred first), `dandi` |

What follows:

- **Two to four content words.** A sentence finds fewer datasets than its keywords at every "every word" source.
- **Plain words, no operators.** Quotes, `AND`, `field:value` and wildcards mean different things in Elasticsearch, Solr, Entrez and PANGAEA syntax, and nothing in a local catalog.
- **The archive's own vocabulary.** A name-only source finds "mnist" and not "handwritten digits"; a second query with the dataset's name or acronym covers it.
- **Languages.** `europa` matches across EU languages. `govdata`, `recherche-data-gouv`, `data-go-jp` and `modelscope` hold much metadata in German, French, Japanese and Chinese; a query in that language can answer better there.

The rules for a whole search and the measured effect: `references/search.md`, Writing queries.

## Local catalogs

A `local` source publishes a complete list and no search endpoint. dataseek downloads the list, keeps it 7 days, and ranks it itself: every query word must start a token, a title match counts three times, and the whole query in the title wins.

Consequences for a search:

- A local source answers in milliseconds and never rate-limits a query.
- Its first use, or its use after 7 days, shows as `downloading its catalog` or `expired catalog` in `sources[].status`. `dsk cache warm` downloads them all once (`references/setup.md`, Cache).
- It matches only what the list carries. `opus` and `tfds` carry names; a topic word finds nothing there.

Sizes, as entries in the list (2026-10-06 unless dated):

| Source | Entries | Source | Entries |
|---|---|---|---|
| `worldbank` | 29,533 | `pwc` | 11,265 (2026-11-23) |
| `eurostat` | 7,563 | `openml` | 6,668 (2026-10-20) |
| `who` | 3,099 | `neurips` | 2,411 (2026-11-23) |
| `nomad` | 2,121 | `openneuro` | 1,905 |
| `census` | 1,809 | `academictorrents` | 1,724 (2026-11-23) |
| `oecd` | 1,548 | `ilo` | 1,216 |
| `aws` | 1,215 | `opus` | 1,215 (2026-11-23) |
| `earth-engine` | 867 | `uci` | 689 |
| `swisstopo` | 515 (2026-11-23) | `physionet` | 477 |
| `tfds` | 446 | `copernicus-dataspace` | 427 |
| `cellxgene` | 397 | `grandchallenge` | 264 (2026-11-23) |
| `tcia` | 241 (2026-11-23) | `imf` | 222 |
| `ecb` | 215 | `googleresearch` | 169 (2026-11-23) |
| `openslr` | 152 (2026-11-23) | `copernicus-cds` | 144 |
| `planetary-computer` | 138 | `polaris` | 96 (2026-11-23) |
| `bundesbank` | 94 | `materials-project` | 72 |
| `lila` | 59 (2026-11-23) | `unicef` | 56 (2026-11-23) |
| `fiscal-data` | 56 | `bis` | 32 |
| `undata` | 15 | `usgs-landsat` | 14 (2026-11-23) |
| `earth-search` | 9 | | |

A small list is a reason to search it with one word: `bis` has 32 tables, and `source: ["bis"]` with "credit" returned its two credit tables.

## Slow, limited and keyed sources

A default search stops one second after 90% of the sources have answered, so a slow source is `not waited for` and its results are missing. On 2026-11-23, "sea surface temperature" over the default set did not wait for `openaire`, `b2find`, `huggingface`, `juelich`, `govdata`, `arrayexpress`, `biostudies`, `bioimages`, `empiar` and `omicsdi`. Name a source in `source` and it gets the whole `timeout`.

| Sources | What happens | What to do |
|---|---|---|
| `arrayexpress`, `biostudies`, `bioimages`, `empiar` | EBI Search took 25 to 60 seconds a request on 2026-11-23; a default search never has them | when the topic is theirs, `source` set to them and `timeout: 150`; two of them answered in 54 and 61 seconds that way |
| `openaire`, `b2find`, `omicsdi`, `govdata`, `juelich` | slower than the quorum on 2026-11-23, so not waited for | `source` set to the one that matters for the topic |
| `huggingface` | a query of several words also searches the dataset cards, limited to 50 such searches per 5 minutes, with or without a key; then `rate limited by the source` | one-word queries for a known name; wait a few minutes after the limit |
| `github` | 10 searches a minute without `GITHUB_TOKEN`, 30 with | set the key for a batch |
| `gitlab` | 60 requests per clock hour per address; dataseek reads no GitLab key | wait for the next full hour |
| `osf` | an hour-long refusal after 17 requests in 3 minutes was seen | do not loop queries with `source: ["osf"]` |
| `zenodo` | 30 searches a minute | rarely reached; the cache answers a repeat within 6 hours |
| `fred`, `roboflow`, `datacommons` | skipped with `needs $VAR` until the key is set | `references/setup.md`, Keys |
| `kaggle`, `datagov`, `geo` | work without a key; a key raises limits for `datagov` and `geo` | nothing |

A missing source matters only when the topic belongs to it. Report it in Coverage; search again with `source` only when it matters.

## Opt-in sources and notices

- **`google` and `mendeley`** are never asked by a default search, a category or an empty `source`. Name one only when the user names it. Google has no API, so dataseek reads its results page. Mendeley's terms bar automated access without written permission; tell the user that before the search.
- **`fred` and `census`** carry a `notice`. When an answer uses results from one of them, add its notice line once, as the `sources` row gives it.
- **WHO** asks for a citation of the form "World Health Organization {YEAR} data.who.int, {ITEM_NAME} [{ITEM_TYPE}]. {URL} (Accessed on {ACCESS DATE})" when its data is reused. Give the user that form when they plan to publish with `who` data.
- Every result carries its `publisher` and `url`; keep both in an answer, since most providers ask that the source is named when data is reused.

## The answer

```text
Sources
1. empiar (EMPIAR, life-sciences, live, no key): raw electron-microscopy images
2. bioimages (BioImage Archive, life-sciences, live, no key): biological images of every modality

Checked: sources, 106 rows; both are EBI Search domains, so a search of them needs `source` and `timeout: 150`.
```

**Sources** lists only the rows that answer the question, each with its `id`, `name`, `category`, `search` and `key` as the row gives them. **Checked** names the call and the row count, and anything the user must know before searching those sources: a missing key, an opt-in rule, a slow archive.
