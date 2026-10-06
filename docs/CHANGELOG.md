# Changelog

What shipped, newest first. release-plz writes each section from the conventional commits merged since the previous tag; see [`reference/release.md`](reference/release.md).

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
