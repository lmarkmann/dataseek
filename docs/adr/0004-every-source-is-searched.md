# ADR 0004: Every source is searched, none is tiered

- Status: accepted
- Date: 2026-10-06

## Context

Google Dataset Search was dataseek's only source in the lost Python version, and it misses whole classes of data: Hugging Face pages carry no schema.org markup, statistical series and STAC collections rarely do, and omics accessions live behind their own archives. A research pass on 2026-10-06 ranked about sixty candidate sources into tiers. Tiers would have decided by guess which sources earn an adapter; the alternative is to build them all to one standard and let measurement decide.

## Decision

Every source below is implemented to the same contract (`src/sources.rs`) and searched by default, except the opt-in ones ([ADR 0013](0013-opt-in-sources.md)), which run only when `--source` names them. A source is left out of a run only when it needs a key the user has not set, when it had an outage in the last ten minutes, or when the user narrows the run with `--source`, `--exclude` or `--category`. `dataseek bench` measures latency, answer rate, result count and overlap per source, and that measurement, not a tier list, is what a source is later demoted or dropped on.

`search` is `live` when the query goes to the source, `local` when the source publishes a complete list but no search endpoint, so its catalog is downloaded (cached for a week) and searched on disk.

## Consequences

- One query fans out to about 74 hosts. The search deadline (ADR 0007) bounds the cost of the slowest; the cache bounds repeats.
- Adding a source is one registry row plus, at most, one module. Removing one is the reverse; nothing else refers to a source by id.
- Overlap is low (ADR 0006 evidence: 96% of merged hits came from exactly one source), so dropping a source loses results rather than duplicates. Reverting to tiers would cut recall in proportion.

## Evidence

- `dataseek bench` on 2026-10-06, six queries, 76 sources: all 74 that had their keys answered 6 of 6; median latency per source from 0 ms (local catalogs) to 1.4 s (data.gov.au). Full table: [`../synthesis/bench-2026-10-06.md`](../synthesis/bench-2026-10-06.md).
- Search semantics per interface: [`../synthesis/search-methods.md`](../synthesis/search-methods.md).

## The sources

76 sources, grouped by `--category`. Each source's terms verdict and the sentence it rests on are in [`../synthesis/terms.md`](../synthesis/terms.md); what each offers for filters and file lists is in [`../synthesis/capabilities.md`](../synthesis/capabilities.md). Keys are read from the environment or `credentials.toml` ([`../reference/keys-and-cache.md`](../reference/keys-and-cache.md)); an optional key raises a rate limit or unlocks a better endpoint, a required one is the only way in.

### aggregator

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `datacite` | DataCite | DataCite REST | live |  | [docs](https://support.datacite.org/docs/api) |
| `openaire` | OpenAIRE Graph | OpenAIRE Graph | live |  | [docs](https://graph.openaire.eu/docs/apis/graph-api/) |
| `google` | Google Dataset Search | results page data | live | opt-in | [docs](https://datasetsearch.research.google.com/help) |
| `b2find` | EUDAT B2FIND | CKAN | live |  | [docs](https://docs.ckan.org/en/latest/api/) |

### machine-learning

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `huggingface` | Hugging Face Hub | Hub API | live | `$HF_TOKEN` (optional) | [docs](https://huggingface.co/docs/huggingface_hub/package_reference/hf_api) |
| `kaggle` | Kaggle | Kaggle API | live | `$KAGGLE_API_TOKEN` (optional) | [docs](https://www.kaggle.com/docs/api) |
| `openml` | OpenML | OpenML REST, listed | local |  | [docs](https://docs.openml.org/ecosystem/Rest/) |
| `uci` | UCI Machine Learning Repository | list endpoint | local |  | [docs](https://github.com/uci-ml-repo/ucimlrepo) |
| `roboflow` | Roboflow Universe | Universe API | live | `$ROBOFLOW_API_KEY` (required) | [docs](https://docs.roboflow.com/datasets/universe/universe/universe-search) |
| `modelscope` | ModelScope | site API | live |  | [docs](https://www.modelscope.cn/docs) |
| `aws` | Registry of Open Data on AWS | registry page | local |  | [docs](https://github.com/awslabs/open-data-registry) |
| `tfds` | TensorFlow Datasets | catalog page | local |  | [docs](https://www.tensorflow.org/datasets/catalog/overview) |

### code

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `github` | GitHub (topic:dataset) | GitHub search | live | `$GITHUB_TOKEN` (optional) | [docs](https://docs.github.com/en/rest/search/search) |

### research

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `zenodo` | Zenodo | InvenioRDM | live |  | [docs](https://developers.zenodo.org/) |
| `figshare` | Figshare | Figshare | live |  | [docs](https://docs.figshare.com/) |
| `harvard-dataverse` | Harvard Dataverse | Dataverse | live |  | [docs](https://guides.dataverse.org/en/latest/api/search.html) |
| `borealis` | Borealis (Canada) | Dataverse | live |  | [docs](https://guides.dataverse.org/en/latest/api/search.html) |
| `recherche-data-gouv` | Recherche Data Gouv (France) | Dataverse | live |  | [docs](https://guides.dataverse.org/en/latest/api/search.html) |
| `dataverse-nl` | DataverseNL | Dataverse | live |  | [docs](https://guides.dataverse.org/en/latest/api/search.html) |
| `dataverse-no` | DataverseNO | Dataverse | live |  | [docs](https://guides.dataverse.org/en/latest/api/search.html) |
| `osf` | OSF (via SHARE) | SHARE trove | live |  | [docs](https://share.osf.io/trove/docs) |
| `mendeley` | Mendeley Data | site search API | live | opt-in | [docs](https://data.mendeley.com/api/docs/) |

### government

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `europa` | data.europa.eu | DCAT-AP (piveau) | live |  | [docs](https://dataeuropa.gitlab.io/data-provider-manual/api-documentation/) |
| `datagov` | Data.gov | Data.gov Catalog API | live | `$DATAGOV_API_KEY` (optional) | [docs](https://resources.data.gov/catalog-api/) |
| `data-gov-uk` | data.gov.uk | CKAN | live |  | [docs](https://docs.ckan.org/en/latest/api/) |
| `open-canada` | Open Government Canada | CKAN | live |  | [docs](https://docs.ckan.org/en/latest/api/) |
| `data-gov-au` | data.gov.au | CKAN | live |  | [docs](https://docs.ckan.org/en/latest/api/) |
| `govdata` | GovData (Germany) | CKAN | live |  | [docs](https://docs.ckan.org/en/latest/api/) |
| `hdx` | Humanitarian Data Exchange | CKAN | live |  | [docs](https://docs.humdata.org/build/hdx-apis/metadata-endpoints/package_search) |
| `socrata` | Socrata portals (US) | Socrata Discovery | live |  | [docs](https://dev.socrata.com/docs/other/discovery) |
| `socrata-eu` | Socrata portals (EU) | Socrata Discovery | live |  | [docs](https://dev.socrata.com/docs/other/discovery) |
| `opendatasoft` | OpenDataSoft hub | OpenDataSoft Explore | live |  | [docs](https://help.huwise.com/apis/ods-explore-v2/) |
| `arcgis` | ArcGIS Hub | OGC API Records | live |  | [docs](https://hub.arcgis.com/api/search/v1) |

### statistics

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `ihsn` | IHSN survey catalog | NADA | live |  | [docs](https://microdata.worldbank.org/api-documentation/catalog/index.html) |
| `fao-microdata` | FAO Microdata | NADA | live |  | [docs](https://microdata.worldbank.org/api-documentation/catalog/index.html) |
| `unhcr-microdata` | UNHCR Microdata Library | NADA | live |  | [docs](https://microdata.worldbank.org/api-documentation/catalog/index.html) |
| `eurostat` | Eurostat | Eurostat table of contents | local |  | [docs](https://ec.europa.eu/eurostat/web/user-guides/data-browser/api-data-access) |
| `undata` | UNdata | SDMX | local |  | [docs](https://data.un.org/Host.aspx?Content=API) |
| `datacommons` | Data Commons | Data Commons REST v2 | live | `$DATACOMMONS_API_KEY` (required) | [docs](https://docs.datacommons.org/api/rest/v2/) |
| `owid` | Our World in Data | Search API | live |  | [docs](https://docs.owid.io/projects/etl/api/search-api/) |
| `census` | U.S. Census Bureau API | DCAT data.json, listed | local |  | [docs](https://census.gov/data/developers/updates/new-discovery-tool.html) |
| `who` | WHO Global Health Observatory | OData, listed | local |  | [docs](https://www.who.int/data/gho/info/gho-odata-api) |

### economics

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `dbnomics` | DBnomics | DBnomics | live |  | [docs](https://api.db.nomics.world/v22/apidocs) |
| `worldbank` | World Bank indicators | World Bank API, listed | local |  | [docs](https://datahelpdesk.worldbank.org/knowledgebase/articles/889392) |
| `worldbank-microdata` | World Bank Microdata Library | NADA | live |  | [docs](https://microdata.worldbank.org/api-documentation/catalog/index.html) |
| `imf` | IMF | SDMX | local |  | [docs](https://data.imf.org/en/Resource-Pages/IMF-API) |
| `oecd` | OECD | SDMX | local |  | [docs](https://www.oecd.org/en/data/insights/data-explainers/2024/09/api.html) |
| `ilo` | ILOSTAT | SDMX | local |  | [docs](https://www.ilo.org/resource/other/ilostat-sdmx-user-guide) |

### finance

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `ecb` | European Central Bank | SDMX | local |  | [docs](https://data.ecb.europa.eu/help/api/overview) |
| `bis` | Bank for International Settlements | SDMX | local |  | [docs](https://stats.bis.org/api-doc/v2/) |
| `fred` | FRED | FRED API | live | `$FRED_API_KEY` (required) | [docs](https://fred.stlouisfed.org/docs/api/fred/series_search.html) |
| `bundesbank` | Deutsche Bundesbank | SDMX | local |  | [docs](https://statistiken.bundesbank.de/content/991208) |
| `fiscal-data` | U.S. Treasury Fiscal Data | Fiscal Data API, listed | local |  | [docs](https://fiscaldata.treasury.gov/api-documentation/) |

### geospatial

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `cmr` | NASA Earthdata (CMR) | CMR search | live |  | [docs](https://cmr.earthdata.nasa.gov/search/site/docs/search/api.html) |
| `planetary-computer` | Microsoft Planetary Computer | STAC | local |  | [docs](https://planetarycomputer.microsoft.com/docs/reference/stac/) |
| `earth-search` | Earth Search (AWS) | STAC | local |  | [docs](https://element84.com/earth-search/) |
| `copernicus-dataspace` | Copernicus Data Space | STAC | local |  | [docs](https://documentation.dataspace.copernicus.eu/APIs/STAC.html) |
| `copernicus-cds` | Copernicus Climate Data Store | STAC | local |  | [docs](https://cds.climate.copernicus.eu/how-to-api) |
| `earth-engine` | Google Earth Engine catalog | catalog page | local |  | [docs](https://developers.google.com/earth-engine/datasets/catalog) |
| `ncei` | NOAA NCEI | NCEI Search Service | live |  | [docs](https://www.ncei.noaa.gov/support/access-search-service-api-user-documentation) |
| `pangaea` | PANGAEA | PANGAEA search | live |  | [docs](https://wiki.pangaea.de/wiki/PANGAEA_search) |

### ecology

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `gbif` | GBIF | GBIF registry | live |  | [docs](https://techdocs.gbif.org/en/openapi/v1/registry) |
| `dataone` | DataONE | DataONE Solr | live |  | [docs](https://dataoneorg.github.io/api-documentation/) |

### life-sciences

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `geo` | NCBI GEO | E-utilities | live | `$NCBI_API_KEY` (optional) | [docs](https://www.ncbi.nlm.nih.gov/books/NBK25501/) |
| `arrayexpress` | ArrayExpress (EBI) | EBI Search | live |  | [docs](https://www.ebi.ac.uk/ebisearch/documentation/rest-api) |
| `biostudies` | BioStudies (EBI) | EBI Search | live |  | [docs](https://www.ebi.ac.uk/ebisearch/documentation/rest-api) |
| `omicsdi` | OmicsDI | OmicsDI | live |  | [docs](https://www.omicsdi.org/ws/) |
| `cellxgene` | CZ CELLxGENE | Curation API, listed | local |  | [docs](https://api.cellxgene.cziscience.com/curation/ui/) |
| `synapse` | Synapse | Synapse REST | live |  | [docs](https://rest-docs.synapse.org/) |

### neuroscience

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `openneuro` | OpenNeuro | GraphQL, listed | local |  | [docs](https://docs.openneuro.org/api.html) |
| `dandi` | DANDI Archive | DANDI REST | live |  | [docs](https://api.dandiarchive.org/swagger/) |
| `physionet` | PhysioNet | project list | local |  | [docs](https://physionet.org/about/) |

### physics

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `cern` | CERN Open Data | Invenio | live |  | [docs](https://github.com/cernopendata/opendata.cern.ch) |
| `materials-project` | Materials Project (MPContribs) | MPContribs, listed | local |  | [docs](https://contribs-api.materialsproject.org/) |
| `nomad` | NOMAD | NOMAD API, listed | local |  | [docs](https://nomad-lab.eu/prod/v1/api/v1/extensions/docs) |

### social-science

| id | source | protocol | search | key | API docs |
|---|---|---|---|---|---|
| `cessda` | CESSDA Data Catalogue | CESSDA | live |  | [docs](https://api.tech.cessda.eu/) |
