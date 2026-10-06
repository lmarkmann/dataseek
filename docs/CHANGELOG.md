# Changelog

What shipped, newest first. release-plz writes each section from the conventional commits merged since the previous tag; see [`reference/release.md`](reference/release.md).

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
