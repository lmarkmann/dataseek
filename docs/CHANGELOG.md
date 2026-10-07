# Changelog

What shipped, newest first. release-plz writes each section from the conventional commits merged since the previous tag; see [`reference/release.md`](reference/release.md).

## 0.7.0 - 2026-10-07

### Changed

- The overview says dsk mcp serves search, sources and inspect, and relayed stderr lines share the event writer
- MCP argument types are a fixed set, and a count flag takes how many times to repeat it

### Docs

- MCP mode's errors, limits and lifetime in the contract, ADR 0015 sizes under the current release profile, and the inspect fixtures as they are
- What just bloat --what-if measures, and size figures that match their byte counts
- Binary size per target before and after, with every lever measured

### Fixed

- Dsk mcp runs at most four calls at once and refuses a fifth as a tool error
- A cancelled dsk mcp call can no longer collect a later call that reuses its request id
- Dsk mcp keeps running its calls after an upgrade replaces the binary
- Dsk mcp refuses an argument holding a NUL character as a tool error, not a server fault
- Dsk mcp refuses a search timeout over 300 seconds, and tool schemas give the range of every ranged integer
- A usage error under --json keeps the missing argument and the possible values, and dsk mcp names a missing argument
- A dsk mcp call that fails without an error, or succeeds without a JSON object, says what it printed instead
- Dsk mcp stops its running calls when the client closes the pipe, instead of dying of SIGPIPE
- Dsk mcp answers a line that is not UTF-8 or a request without a method, and keeps serving
- MCP clients can pass offline and refresh, whose schema no longer limits a boolean to strings
- --jq names its replacement instead of failing as an unknown flag

### Other

- The changelog marks breaking changes
- Drop dependency features dataseek never uses
- Just bloat runs cargo-bsize

### Performance

- Breaking: Drop --jq; pipe --json into jq instead
- Release binary is 23% smaller: fat LTO, dependencies optimized for size

### Uncategorized

- Merge branch 'mcp-server' into relevance-benchmark
- Merge branch 'platform-certs-owned-merge-file-list' into mcp-server
- Merge branch 'platform-certs-owned-merge-file-list' into mcp-server
- Merge branch 'binary-size' into platform-certs-owned-merge-file-list

## 0.6.0 - 2026-10-06

### Added

- Data Commons needs your own API key and is skipped without one
- Google Dataset Search and Mendeley Data are asked only when named with -s
- OpenAIRE results carry download counts and merge on a record's other DOIs, on the v3 API
- Roboflow Universe returns more than one page of hits and keeps your key out of the URL
- Data.europa.eu results carry the DOI, replies are a fifth the size, and a stray quote no longer fails the search
- STAC results carry the collection's DOI and Data Space links open its browser page
- Hugging Face results carry the DOI and size, and descriptions start with prose instead of the card heading
- DANDI results carry the DOI and star count, skip empty dandisets and list starred ones first; a colon in the query no longer fails the search
- Date ILOSTAT dataflows, drop IMF ones with no portal page
- Kaggle searches read as many pages as the limit asks for
- OpenNeuro results carry a description, DOI, license, size and download count
- TensorFlow Datasets results show the categories they are listed under
- Data.gov results carry monthly views and more DOIs
- Materials Project results carry the full title, license and publisher

### Changed

- GitHub leaves its 403 quota handling to the HTTP client
- Data.gov reads the landing page once when it looks for a DOI
- Fiscal Data reads only the description fields the API sends

### Docs

- PANGAEA row says all words must match
- Record each source's terms verdict and what it offers for filters and file lists
- Search methods, limits and watch list match what the adapters now do
- ADR 0004 rows follow the registry docs links
- Registry rows point at the docs pages that exist today
- NCEI header lists every character the search service rejects
- DANDI header lines stay within 79 columns
- WHO header counts 37 archived indicator stubs
- ArcGIS Hub header spells the throttle header as the service sends it
- Record NOMAD's name filters, paging, rate limit and metadata terms
- DataONE header counts the DOIs and the records without a creator against the current index
- Earth Engine adapter records the catalog page's size, what it lacks and why STAC is not used
- Census header says what its link is and dates the key and attribution rules
- DBnomics header says over 90 providers beside the dated count
- CESSDA adapter records its page limit, result window and why it asks for English
- Socrata adapter records its page limit, throttle and which asset types it keeps
- Record how Synapse search pages, ranks and what it indexes
- OpenML results are described by size because the list carries no text
- DBnomics adapter records its page limit and what the search matches

### Fixed

- Eurostat sizes its description buffer with saturating arithmetic
- A 403 with a spent quota reads as rate limited, not as rejected credentials
- A catalog searched from an expired copy reads stale instead of ok
- An offline search no longer clears a source's outage mark
- A connect timeout you set no longer marks the source as down for ten minutes
- OSF results name OSF as the publisher instead of the project's first author
- PANGAEA drops leading wildcards when it retries a rejected query
- OpenAIRE keeps phrases and operators it accepts and asks again as plain words only after a rejection
- CKAN tries each date field in turn until one parses
- Roboflow fails the search when a later page fails, so a short list is not cached
- Eurostat datasets are found by their code again
- Zenodo keeps phrase and wildcard queries and escapes them only after an HTTP 500
- Data.europa.eu searches asking for more than 1000 results no longer fail
- STAC collections that share a DOI stay separate results
- EBI Search retries a rejected query only when escaping changes it
- SDMX last-update stamps with an impossible day or month are ignored
- PhysioNet results leave out software and models, merge on the concept DOI, and read the current list endpoint
- World Bank indicators outside WDI link to their API record, not a missing page
- AWS Open Data results no longer show a lone ellipsis as the description
- NCEI results no longer carry the end of coverage as their update date, and a query its text parameter rejects is retried as plain words
- PANGAEA results name PANGAEA as publisher instead of the authors, and a query its parser rejects is retried as plain words
- Microdata searches (World Bank, IHSN, FAO, UNHCR) keep surveys whose title lacks the query word
- Google Dataset Search results show the license Google names by code
- DataCite returns matches by relevance instead of newest first, with download counts and unescaped descriptions, and retries a query its parser rejects
- FRED results carry the series id in the title
- Our World in Data returns up to 100 results and claims no license
- TensorFlow Datasets names the missing All Datasets listing when the page changes
- CKAN portals return plain descriptions, real publishers and dataset dates
- CERN Open Data pages through results ten at a time and retries a query its parser rejects
- GBIF licenses read as SPDX ids and the unspecified placeholder is dropped
- DataONE results carry the DOI from the series id and a query with OR or NOT no longer fails
- Zenodo returns up to 100 results per search, and a query with a slash or a lone ! no longer parks it as down
- Eurostat results are described by their topic folders, not by their code
- OpenDataSoft links open the publishing portal and drop the source-link text from descriptions
- Materials Project projects stay findable by their short title
- WHO search no longer lists archived indicator stubs that hold no data
- ArrayExpress and BioStudies accept queries with slashes and brackets, and BioStudies shows abstracts and dates
- ModelScope results carry their size and real update date, and an empty summary stays empty
- OmicsDI leaves out Europe PMC papers and names each repository
- Figshare shows when a dataset was last modified, and a search under 3 characters returns nothing instead of failing
- GEO records stop using organism and sample count as publisher
- Keep CELLxGENE collections of one paper as separate results
- NASA CMR results take their DOI and date from the collection record
- Data Commons uses only your own key and no longer shows a variable id as its description
- A long query no longer parks Mendeley Data as down
- GitHub's update date is the last push, and an exhausted quota reads as a rate limit

### Other

- Keep typos quiet about quoted titles, an SPDX id and test words
- Record what the UCI list endpoint returns, checked in October 2026
- Record the ArcGIS Hub limits and field meanings checked in October 2026

### Performance

- Eurostat builds each description with one allocation
- Eurostat keeps one folder path and trims it by offset, so no row rebuilds it
- Eurostat builds each topic path once per folder instead of once per dataset

## 0.5.0 - 2026-10-06

### Added

- A bare dsk shows an overview; help topics, --jq, --offline and tagged JSON

### Docs

- Help text and contract say what the code does
- Drop leftover template wording

### Fixed

- A failed write to stderr no longer panics
- Inspect no longer calls one unreachable page an offline machine
- Cache warm names every failed catalog and refuses a cache it cannot write
- --json keeps stderr NDJSON under -v, and help <command> and a bare call answer in JSON
- --offline answers from the cache even after an outage, and an empty run says why
- Cache clear removes its own entries, never other files in --cache-dir
- --jq output carries no control characters from remote text
- Cached answers from an older release are refetched, not served as fresh

### Uncategorized

- Merge remote-tracking branch 'origin/main' into cli-conformance

## 0.4.3 - 2026-10-06

### Docs

- Measured gains from the allocation and complexity pass

### Other

- Bench cleaning prose full of bare ampersands

### Performance

- Load cached catalogs with one UTF-8 check
- Parse Eurostat's table of contents without per-line allocations
- Parse SDMX dataflow lists with one pass over each element's attributes
- Merge results with fewer copies and allocations
- Search downloaded catalogs without allocating per entry
- Clean remote text in linear time and with fewer copies

## 0.4.2 - 2026-10-06

### Fixed

- Data.gov descriptions show text instead of raw Markdown
- NCEI results name the organization the record gives
- NASA CMR results name the archive center, not its provider code
- OpenDataSoft results name the source portal when the publisher is blank
- OSF results carry the project's DOI
- OSF results show the project's size
- OSF results show the project's license
- Google Dataset Search results show the license when Google has one
- Google Dataset Search results name the publisher, not the authors
- GitHub results show the repository's size
- GBIF results show when the dataset was last modified
- Fiscal Data results show when the dataset was last updated
- ArcGIS Hub results carry their view count
- ArcGIS Hub results show the dataset's size
- ECB results link to a page that opens for every dataflow
- Earth Engine results show the catalog's description and provider
- DBnomics results show the dataset's description
- Census results show the dataset's CC0 license
- CERN Open Data results show the dataset's size
- CERN Open Data results show the record's license
- PhysioNet results show the project's size
- PhysioNet results show the project's license
- CESSDA results carry the study's DOI
- Synapse and STAC descriptions show text instead of raw Markdown
- Socrata descriptions no longer carry zero-width spaces

## 0.4.1 - 2026-10-06

### Changed

- Split each adapter's parsing from its request

### Docs

- How the adapters, loop and HTTP client are tested

### Fixed

- Drop ECB dataflows that have no page of their own
- Send Mendeley Data the query it actually reads
- Cut World Bank descriptions on a word and mark the cut
- Decode named and numeric HTML entities in remote text
- Pick the AWS registry's description paragraph by shape
- Keep GBIF's real datasets in DataCite results
- Treat ArcGIS Hub's "none" license as no license
- Strip every Google tracking fragment from result links
- Take CMR's DOI from its links and stop dating by coverage start
- Date a dandiset by the version its name and size come from
- Read PhysioNet's publish_date
- Spell out compact dates like 20100708 for every source
- Keep OpenML's catalog in a stable order
- Read numbers written as decimal strings
- End a DOI at the first space
- Report a GEO esummary without results as a changed response
- Never cache an empty catalog, fall back to the old one instead
- Keep a literal < in titles instead of eating the rest
- Strip control characters from localized text

### Other

- Keep typos off recorded fixtures

### Uncategorized

- Merge remote-tracking branch 'origin/main' into test/source-adapters

## 0.4.0 - 2026-10-06

### Added

- Inspect a page's schema.org or Croissant metadata
- Bench sources for latency and overlap
- Economics and finance categories, with Bundesbank, Treasury Fiscal Data and Census
- Add CERN Open Data, MPContribs, NOMAD and CESSDA
- Add OpenNeuro, DANDI and PhysioNet
- Add NCBI GEO, OmicsDI, CELLxGENE and Synapse
- Add GBIF and DataONE
- Add NASA CMR, Earth Engine, NOAA NCEI and PANGAEA
- Add DBnomics, World Bank, Eurostat, Data Commons, OWID, FRED and WHO
- Add Data.gov, OpenDataSoft and ArcGIS Hub
- Add OSF and Mendeley Data
- Add AWS Open Data, TensorFlow Datasets and GitHub
- Search Google Dataset Search
- Search the SDMX dataflows of statistical agencies
- Search STAC collection catalogs
- Search EMBL-EBI archives through EBI Search
- Search Socrata portals through the Discovery API
- Search NADA microdata catalogs
- Search Dataverse installations
- Search CKAN portals
- Search datasets across sources with merged, deduplicated results
- Ship the same program as dsk

### Changed

- Drop the template's count command

### Docs

- Source list, design ADRs, search methods and bench results

### Fixed

- Report a crashed adapter as crashed, not as still running
- Keep one source's same-named records apart and give Data Commons unique links
- Strip control characters from remote metadata before printing

### Other

- Use the platform TLS stack on Windows and macOS
- Gitignore from the GitHub Rust template
- Trim comments in src and tests ([#8](https://github.com/lmarkmann/dataseek/pull/8))
- Docs layout, release-plz config under .github, current pins ([#6](https://github.com/lmarkmann/dataseek/pull/6))

## 0.3.1 - 2026-09-25

No user-facing change; the release workflow cut this version from two `ci:` merges.

## 0.3.0 - 2026-09-09

First tagged version.
