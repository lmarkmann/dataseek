# Search methods across the sources

dataseek asks every source for `--per-source` results (default 10, at most 100), so the page caps below matter only for `bench` and future paging.

How each search interface takes a query, pages, ranks and limits, as found when the adapters were written (2026-10-06). The adapter for each row lives in `src/sources/`. What dataseek does with the differences is decided in ADRs 0005 to 0008.

## Query semantics

| interface | method and endpoint | query parameter | matching | ranking | documented page cap |
|---|---|---|---|---|---|
| Hugging Face Hub | GET `/api/datasets` | `search` | substring of the repo id only | `sort=downloads` (no relevance) | not documented |
| Kaggle | GET `/api/v1/datasets/list` | `search` | title, subtitle, tags | Kaggle "hottest" by default | 20, fixed (observed) |
| DataCite | GET `/dois` | `query` (Elasticsearch query string) | all metadata fields | Elasticsearch relevance, no popularity | 1,000 |
| OpenAIRE Graph | GET `/graph/v1/researchProducts` | `search` | title, abstract, subjects | relevance | not documented |
| Google Dataset Search | GET `/search` (HTML) | `query` | Google's | Google's | 20 (observed) |
| Zenodo (InvenioRDM) | GET `/api/records` | `q` (Elasticsearch syntax) | all fields | `bestmatch` | 25 anonymous, 100 with token |
| Figshare | POST `/v2/articles/search` | `search_for` in a JSON body | all fields, field operators like `:title:` | relevance | 1,000; offset capped at 1,000 |
| Dataverse | GET `/api/search` | `q` (Solr syntax) | all fields | Solr relevance | 1,000 |
| CKAN | GET `/api/3/action/package_search` | `q` (Solr syntax) | all fields | Solr relevance | not documented |
| data.europa.eu | GET `/api/hub/search/search` | `q`, `filter=dataset` | stemmed, multilingual | relevance | not documented |
| Data.gov | GET `/search` or `api.gsa.gov/.../v4/search` | `q` | OpenSearch full text | relevance; cursor `after` | not documented |
| Socrata Discovery | GET `/api/catalog/v1` | `q`, `only=dataset` | name, description, columns | relevance | 10,000 |
| OpenDataSoft | GET `/api/explore/v2.1/catalog/datasets` | `where=search("...")` (ODSQL) | all text fields | relevance | 100 |
| ArcGIS Hub | GET `/api/search/v1/collections/dataset/items` | `q` (OGC API Records) | title, tags, description | relevance | `numberMatched` caps at 10,000 (observed) |
| DBnomics | GET `/v22/search` | `q` | dataset names and codes | relevance | not documented |
| NADA | GET `/api/catalog/search` | `sk` | any word, loose | relevance | not documented |
| NASA CMR | GET `/search/collections.json` | `keyword` | all collection metadata | relevance | 2,000 |
| NOAA NCEI | GET `/access/services/search/v1/datasets` | `text` | full text | relevance | not documented |
| PANGAEA | GET `/advanced/search.php` | `q` | full text | score per hit | not documented |
| GBIF | GET `/v1/dataset/search` | `q` | full text | relevance | 1,000 |
| DataONE | GET `/cn/v2/query/solr/` | Solr `q`, `fq` | Solr fields | Solr relevance | not documented |
| NCBI GEO | GET `esearch.fcgi` then `esummary.fcgi` | `term` (Entrez syntax) | indexed fields, `[ETYP]` filters | Entrez relevance | not documented |
| EBI Search | GET `/ebisearch/ws/rest/{domain}` | `query` | domain fields | relevance | 100 |
| OmicsDI | GET `/ws/dataset/search` | `query` | full text | relevance | not documented |
| Synapse | POST `/repo/v1/search` | `queryTerm` list, `booleanQuery` | all words | relevance | not documented |
| DANDI | GET `/api/dandisets/` | `search` | name, description | relevance | not documented |
| CERN Open Data | GET `/api/records/` | `q`, `type=Dataset` | Invenio | relevance | not documented |
| CESSDA | GET `/api/DataSets/v2/search` | `q`, mandatory `metadataLanguage` | full text | relevance | not documented |
| Mendeley Data | GET `/api/research-data/search` | `search` | full text | relevance | not documented |
| OSF via SHARE | GET `/trove/index-card-search` | `cardSearchText`, IRI-valued `cardSearchFilter` | full text | relevance | not documented |
| Data Commons | GET `/v2/resolve` | `nodes`, `resolver=indicator` | semantic match to statistical variables | similarity score | not documented |
| Our World in Data | GET `/api/search` | `q` | charts and articles | relevance | not documented |
| FRED | GET `/fred/series/search` | `search_text` | full text | relevance or popularity | 1,000 |
| GitHub | GET `/search/repositories` | `q` with `topic:dataset` | name, description, README | best match | 100 |
| ModelScope | GET `/api/v1/dolphin/datasets` | `Query` | names | site default | not documented |
| Roboflow Universe | GET `/universe/search` | `q`, inline filters (`images>100`) | names, classes | site default | not documented |

## Catalogs searched locally

These publish a complete list but no search endpoint, so dataseek downloads the list (weekly) and ranks it itself (`src/catalog.rs`: every word must match a token prefix; title matches count three times; the whole query in the title wins).

| source | list endpoint | entries (2026-10-06) |
|---|---|---|
| OpenML | `/api/v1/json/data/list/limit/20000/status/active` (latest version per name) | 5,025 |
| UCI | `/api/datasets/list` | 689 |
| World Bank indicators | `/v2/indicator?per_page=40000` | 29,533 |
| SDMX agencies | `/dataflow` as SDMX-ML 2.1 | IMF 222, OECD 1,548, ECB 215, BIS 32, ILO 1,216, UNdata 15, Bundesbank 94 |
| Eurostat | catalogue table of contents (`toc/txt`) | 7,563 |
| WHO GHO | OData `/api/Indicator` | 3,099 |
| STAC catalogs | `/collections?limit=1000`, following `next` | Planetary Computer 138, Earth Search 9, Copernicus Data Space 427, CDS 144 |
| Earth Engine | catalog HTML page | 867 |
| AWS Open Data | registry index HTML page | 1,214 |
| TensorFlow Datasets | catalog overview links | 446 |
| CELLxGENE | Curation API `/collections` | 397 |
| OpenNeuro | GraphQL `datasets`, cursor pages of 100 | 1,905 |
| PhysioNet | `/api/v1/project/published/` | 545 |
| MPContribs | `/projects/` | 72 |
| NOMAD | `/datasets/`, `page_after_value` paging | 2,120 |
| Treasury Fiscal Data | `/services/dtg/metadata/` | 56 |
| Census Bureau | DCAT `data.json` | 1,809 |

## Limits and etiquette

| source | limit without a key | with a key |
|---|---|---|
| Hugging Face | 500 requests per 5 minutes per IP | 1,000 (free), 2,500 (PRO) |
| Kaggle | dynamic, 429 on abuse | same, attributed to the account |
| DataCite | 500 per 5 minutes; 1,000 with a contact address | 3,000 for members |
| Zenodo | 30 searches per minute | 30 per minute, larger pages |
| Figshare | no hard limit; asks for at most 1 request per second | same |
| OpenAIRE | 7,199 per hour observed | higher with a personal token |
| api.data.gov | `DEMO_KEY`: 30 per hour, 50 per day | 1,000 per hour |
| NCBI E-utilities | 3 per second, with `tool` and `email` | 10 per second |
| GitHub search | 10 per minute | 30 per minute |
| OpenAlex (not used) | $0.10 per day of search credit | $1 per day free, then paid |

## Quirks that shaped the code

- data.gov retired its CKAN API in 2025; both the documented Catalog API and catalog.data.gov's own `/search` return the same JSON.
- `cn.dataone.org` renegotiates TLS on `/cn/` paths to ask for an optional client certificate, which rustls refuses; `search.dataone.org` serves the same index.
- OSF has no dataset resource type; projects and registrations are searched, and SHARE filters take IRIs, not bare names.
- OpenNeuro's GraphQL `search` returns nothing anonymously; the full list is paged instead.
- DataCite's dataset type is dominated by machine events: GBIF mints a DOI per download (4.73 million), CCDC per crystal structure (1.26 million). Both clients are dropped.
- Papers with Code's API redirects to Hugging Face trending papers since July 2025.
