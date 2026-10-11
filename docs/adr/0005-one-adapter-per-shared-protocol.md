# ADR 0005: One adapter per shared protocol

- Status: accepted
- Date: 2026-10-06

## Context

Most dataset portals do not have APIs of their own: they run one of a handful of platforms. Writing per-portal code would multiply adapters by the number of installations.

## Decision

A protocol is written once and configured per registry row:

| protocol | module | rows today | searched |
|---|---|---|---|
| CKAN `package_search` | `ckan.rs` | data.gov.uk, Canada, Australia, GovData, HDX, B2FIND | live |
| Dataverse Search API | `dataverse.rs` | Harvard, Borealis, Recherche Data Gouv, DataverseNL, DataverseNO | live |
| NADA catalog search | `nada.rs` | World Bank, IHSN, FAO, UNHCR | live |
| Socrata Discovery | `socrata.rs` | US and EU regions (every Socrata domain) | live |
| EBI Search | `ebi.rs` | ArrayExpress, BioStudies | live |
| STAC collections | `stac.rs` | Planetary Computer, Earth Search, Copernicus Data Space, Copernicus CDS | local |
| SDMX-ML 2.1 dataflows | `sdmx.rs` | IMF, OECD, ECB, BIS, ILO, UNdata, Bundesbank | local |
| DCAT-AP | `europa.rs` | every EU national portal, through data.europa.eu | live |
| DataCite, OpenAIRE | `datacite.rs`, `openaire.rs` | thousands of DOI and OAI-PMH repositories | live |

Adding an installation of a known protocol is a registry row and nothing else. Protocols that cannot be searched at query time are not used for search: OAI-PMH is harvest-only and would need a local index larger than the cache budget (watch list), and schema.org `Dataset` and Croissant are per-page metadata, read by `dataseek inspect` rather than searched.

Where one agency's protocol endpoint is impractical, the agency gets its own small adapter: Eurostat's SDMX dataflow list is 37 MB of multilingual annotations, so its 2 MB table of contents is read instead (`eurostat.rs`).

## Consequences

- A protocol bug affects every row using it; a protocol fix repairs them all.
- The CKAN registry is curated, not imported: the public registry of 631 portals includes dead and test instances.
- Reverting to per-portal code would cost one module per installation for no capability.

## Evidence

- Probed live 2026-10-06: each listed endpoint answered keyless with the shape the module parses (see `../synthesis/search-methods.md`).
- The Eurostat dataflow list measured 37,664,029 bytes; the table of contents about 2 MB with 10,311 dataset and table rows.
