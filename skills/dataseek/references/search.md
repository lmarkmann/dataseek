# Search

Verified against `tools/list`, `dsk help search --json`, `dsk sources --json` and live searches on 2026-10-20.

## Contents

- **Arguments**: which argument, on which surface, and when each one is worth setting
- **Filters**: license, date, format and agreement constraints, and what an empty filtered result means
- **Source ids and categories**: what to put in `source`, `exclude` and `category`
- **Writing queries**: how long a query should be and what stays out of it
- **Reading the result**: what each field means and which ones an answer may quote
- **Source statuses**: which `status` values mean a gap in the results
- **Opt-in sources**: when Google Dataset Search and Mendeley Data may be asked
- **More, newer, fresher**: `limit`, `per-source`, `sort`, `refresh`, `offline`
- **The answer**: the Datasets, Coverage and Queries parts, filled in

## Arguments

The MCP tool's input schema is generated from the CLI's flags, so the names match: an MCP argument is the long flag without its dashes. Only `query` is required.

| MCP argument | CLI | Default | Set it when |
|---|---|---|---|
| `query` (string) | the words after `search` | required | always; several words form one query |
| `source` (array of ids) | `-s a,b` | every default source | the user wants those archives alone |
| `include` (array of ids) | `--include a,b` | none | the user wants an archive asked beside the default set; also the way to add an opt-in source |
| `exclude` (array of ids) | `-x a,b` | none | one source floods the list with near-duplicates |
| `category` (array of names) | `-c a,b` | every category | the domain is unmistakable (economics, life-sciences) |
| `limit` (integer) | `-n N` | 20 | the user wants more or fewer merged results |
| `license` (array of ids) | `--license a,b` | none | the user needs a license: SPDX ids (`CC-BY-4.0`, `CC0-1.0`) or a family (`cc-by`, every version and port) |
| `updated-since` (`YYYY`, `YYYY-MM` or `YYYY-MM-DD`) | `--updated-since 2024` | none | the user needs data updated on or after a date |
| `format` (array of names) | `--format csv,parquet` | none | the user needs a file format: `csv`, `json`, `parquet`, `netcdf`, `geotiff`, ... |
| `min-sources` (integer, at least 1) | `--min-sources 2` | none | only datasets several archives hold are wanted |
| `per-source` (1 to 100) | `--per-source N` | 10, or 30 when a filter is set | a source should contribute deeper results; costs paging on some sources |
| `sort` (`relevance`, `newest`) | `--sort newest` | `relevance` | the user asks for recent or recently updated data |
| `refresh` (boolean) | `--refresh` | false | the user needs answers newer than the 6-hour cache |
| `offline` (boolean) | `--offline` | false | no network, or a repeat over the cache only |
| `timeout` (1 to 300 seconds) | `--timeout N` | 20 | a slow source matters; the CLI also takes 0, wait for all, which MCP refuses |

Over MCP, `source`, `include`, `exclude` and `category` are JSON arrays, `["zenodo", "figshare"]`; the CLI takes the same ids comma-separated. A string where an array belongs is refused with `` `source` takes a list of strings ``.

`source` replaces the default set; `include` adds to it. A search with `source: ["kaggle"]` asks Kaggle alone, one with `include: ["kaggle"]` asks Kaggle and every default source. `category` and `exclude` still narrow an included source: `include: ["google"]` with `category: ["research"]` leaves Google out, because Google is an `aggregator`. A source named in `source` gets the whole `timeout` and is asked even if it had an outage in the last ten minutes.

## Filters

`license`, `updated-since`, `format` and `min-sources` filter the merged list before `limit` cuts it, the same way for every source. A filter reads each result as it is shown: the license and date it displays (the first ones a source stated) and the formats of every source. A result that does not state a field never passes a filter on it. With a filter set each source is asked for 30 results instead of 10, so a filtered search can return more matches than the unfiltered one.

Measured 2026-10-20 on "sea surface temperature", 328 matches unfiltered: `license: ["cc-by"]` kept 160, `updated-since: "2024"` 488, `format: ["netcdf"]` 37, `min-sources: 2` 10.

- **Licenses are sparse.** About three results in ten name one (315 of 978 over three queries, 2026-10-20), so a license filter drops most of a search. A dropped dataset may still be openly licensed on its page; say so when the filtered list is short, and offer `inspect` on a promising unfiltered result.
- **Formats come from five kinds of source only:** CKAN portals (`data-gov-uk`, `open-canada`, `data-gov-au`, `govdata`, `hdx`), `europa`, `arcgis`, `cmr` and `datagov`. A `format` filter leaves out every research repository; for those, `inspect` the result to read its files.
- **An empty filtered result** says how many matched without the filters (`filtered_out` in the result). Report that number and which filter was strictest rather than "nothing found".
- **`min-sources: 2`** keeps datasets independent archives agree on; it suits a "which datasets are well established" question, not a search for something specific.

## Source ids and categories

The `sources` tool (or `dsk sources --json`) is the live list: id, name, category, protocol, key status and docs URL for each. As of 2026-10-20, by category; `*` marks a catalog source, whose whole list dataseek downloads weekly and searches locally:

| Category | Sources |
|---|---|
| `aggregator` | `datacite`, `openaire`, `google` (opt-in), `b2find` |
| `code` | `github` |
| `ecology` | `gbif`, `dataone` |
| `economics` | `dbnomics`, `worldbank`*, `worldbank-microdata`, `imf`*, `oecd`*, `ilo`* |
| `finance` | `ecb`*, `bis`*, `fred`, `bundesbank`*, `fiscal-data`* |
| `geospatial` | `cmr`, `planetary-computer`*, `earth-search`*, `copernicus-dataspace`*, `copernicus-cds`*, `earth-engine`*, `ncei`, `pangaea` |
| `government` | `europa`, `datagov`, `data-gov-uk`, `open-canada`, `data-gov-au`, `govdata`, `hdx`, `socrata`, `socrata-eu`, `opendatasoft`, `arcgis` |
| `life-sciences` | `geo`, `arrayexpress`, `biostudies`, `omicsdi`, `cellxgene`*, `synapse` |
| `machine-learning` | `huggingface`, `kaggle`, `openml`*, `uci`*, `roboflow`, `modelscope`, `aws`*, `tfds`* |
| `neuroscience` | `openneuro`*, `dandi`, `physionet`* |
| `physics` | `cern`, `materials-project`*, `nomad`* |
| `research` | `zenodo`, `figshare`, `harvard-dataverse`, `borealis`, `recherche-data-gouv`, `dataverse-nl`, `dataverse-no`, `osf`, `mendeley` (opt-in) |
| `social-science` | `cessda` |
| `statistics` | `ihsn`, `fao-microdata`, `unhcr-microdata`, `eurostat`*, `undata`*, `datacommons`, `owid`, `census`*, `who`* |

Map the user's words to ids before narrowing: "Hugging Face" is `huggingface`, "data.gov" is `datagov`, "the World Bank" is `worldbank` (indicators) plus `worldbank-microdata` (surveys), "NASA Earthdata" is `cmr`, "Earth Engine" is `earth-engine`. An id outside this list is refused with the list of valid ones.

A category is a coarse filter. Aggregators such as `datacite` and `openaire` index most research repositories and sit in `aggregator`, so a `category: ["life-sciences"]` search leaves them out; add them with a second, unfiltered search when recall matters more than focus.

## Writing queries

Each source runs its own search engine and most of them require every word to match: the catalog sources need every word as a token prefix, PANGAEA, CESSDA and ModelScope need every word, Socrata needs all of up to three terms, and Hugging Face matches words against the repository id only. So every word added is a filter at most sources.

Measured 2026-10-20 over the default sources: "sea surface temperature" merged 328 results; "global monthly sea surface temperature dataset since 1980" merged 136.

- **Two to four content words.** Drop "dataset", "data", "open", "about", "find me".
- **Constraints stay out of the query.** Licenses, dates and file formats go in the filter arguments (Filters above). A region becomes a second query when it is the topic ("Kenya census"), or is read off `description` afterwards. A year in the query also weighs heavily in the ranking, because rare tokens count most, so "since 2000" lifts tables that merely contain "2000".
- **Use the field's own words and acronyms** as alternative queries: "SST" or "ERSST" beside "sea surface temperature", "scRNA-seq" beside "single cell RNA", "CPI" beside "consumer prices".
- **One concept per query.** "air quality" and "asthma admissions" are two searches whose results the answer joins, not one query that few datasets match on both.
- **English first.** Most sources index English metadata; `europa` matches across EU languages, and a national portal (`govdata`, `recherche-data-gouv`) may answer better in its own language.
- **Run independent queries in parallel**, at most four calls at once over MCP.

## Reading the result

`search` returns one object, schema `dataseek-search/1`:

| Key | What it holds |
|---|---|
| `query` | the query as searched |
| `complete` | false when the search stopped waiting for a source or a catalog was still downloading, so results from those are missing |
| `total` | how many merged datasets matched and passed the filters; `results` holds the first `limit` of them |
| `filtered_out` | how many merged datasets the filters dropped; 0 without filters |
| `results` | the merged datasets, best first, at most `limit` |
| `sources` | one entry per source asked: `id`, `status`, `answered`, `results` (how many it returned), `ms` |

Each result:

| Field | Meaning and use |
|---|---|
| `title`, `url` | always present; `url` is the landing page, the one to give the user and to pass to `inspect` |
| `sources` | every source that holds this dataset; more than one means independent archives agree it exists |
| `description` | cut to a few hundred characters; quote it as a summary, not as the whole record |
| `publisher`, `doi`, `license`, `updated`, `size_bytes` | present only when a source supplied them; `license` is an SPDX id (`CC-BY-4.0`) where the source's spelling maps to one, else the source's own text |
| `formats` | the file formats the sources list, one name each (`CSV`, `NETCDF`); absent when none did |
| `popularity` | downloads, votes or stars as one source counts them; compare only between results from the same source |
| `aliases` | other identifiers of the same dataset (a concept DOI, a mirror URL), used for merging |
| `score` | the rank score inside this one search; meaningless across searches and not a probability |

Several results with the same title from one source are different datasets (one per country, year or version), because only records from different sources merge on title. Tell them apart by URL and description.

## Source statuses

`sources[].status` says why a source did or did not contribute:

| Status | Answered | Meaning |
|---|---|---|
| `ok` | yes | asked now |
| `cached` | yes | answered from the cache, under 6 hours old |
| `stale cache (<reason>)` | yes | the fetch failed, so an older cached answer was used |
| `expired catalog` | yes | a catalog older than 7 days was searched as it is and is downloading again in the background |
| `needs $VAR` | no | the source requires a key that is not set; it was not asked |
| `skipped, outage N s ago` | no | it failed in the last ten minutes; naming it in `source` asks it anyway |
| `not waited for after N s` | no | past the deadline, or past one more second once 90% of the sources had answered |
| `downloading its catalog` | no | its first catalog download is still running and the next search will have it |
| any other text | no | the failure itself, such as `timed out`, `rate limited by the source`, `refused the request (HTTP 403)` |

A missing source matters only when the topic belongs to it: a missing `fred` matters for US economic series and not for protein structures. The fix for each status: `references/troubleshoot.md`.

## Opt-in sources

`google` (Google Dataset Search) and `mendeley` (Mendeley Data) are never asked by default, by a category or by an empty `source`; only naming them in `source` or `include` asks them. Google has no API, so dataseek reads its results page, one request per query; Mendeley's terms bar automated access without written permission. Ask either only when the user names it, and tell the user Mendeley's terms first. `include: ["google"]` asks Google beside every default source in one search, merged and ranked with the rest; `source: ["google"]` asks Google alone.

A server started with `dsk mcp --include google` adds Google to every search that sets neither `source` nor `include` (`references/setup.md`). Results then carry `google` in `sources` without the call naming it; that is the user's configuration, not a breach of rule 3.

## More, newer, fresher

- **More results:** `total` above the length of `results` means more exist; raise `limit` up to `total`.
- **Deeper in one archive:** `source` set to it plus a higher `per-source`.
- **Newest:** `sort: "newest"` orders by `updated`; undated results go last, so say how many lacked a date.
- **Fresh answers:** `refresh: true` skips the 6-hour query cache; the default is right for nearly every call.
- **Many `expired catalog` statuses:** the catalogs are older than a week. `dsk cache warm` downloads them all once (`references/setup.md`).

## The answer

```text
Datasets
1. Extended Reconstructed Sea Surface Temperature (ERSST), Version 4
   https://catalog.data.gov/dataset/extended-reconstructed-sea-surface-temperature-ersst-version-4
   datagov, ncei | NOAA National Centers for Environmental Information | updated 2015-05-26 | CC0 1.0 | doi:10.7289/v5kd1vvf
2. ...

Coverage: 10 of 328 matches shown; 70 of 71 sources answered; complete: false, data-gov-uk not waited for after 2 s.

Queries
dsk search sea surface temperature -n 10
dsk search ERSST -n 10
```

List only the results that fit the request. Constraints the filter arguments cover go in the call; any other constraint is applied to the results, and the answer says how many were dropped and why.
