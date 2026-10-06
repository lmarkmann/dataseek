# Search methods across the sources

dataseek asks every source for `--per-source` results (default 10, at most 100), so a page cap below that matters: Zenodo (25), Kaggle (20), CERN (10 a request), Roboflow (12) and Hugging Face's multi-word scan are paged until the count is met, and the others answer in one request.

How each search interface takes a query, pages, ranks and limits, as found when the adapters were written (2026-10-06). The adapter for each row lives in `src/sources/`. What dataseek does with the differences is decided in ADRs 0005 to 0007 and 0013.

## Query semantics

| interface | method and endpoint | query parameter | matching | ranking | documented page cap |
|---|---|---|---|---|---|
| Hugging Face Hub | GET `/api/datasets` | `search` | substring of the repo id only | `sort=downloads` (no relevance) | 1,000 (observed); `skip` offset or `Link` cursor |
| Kaggle | GET `/api/v1/datasets/list` | `search` | title, subtitle, tags | Kaggle "hottest" by default | 20, fixed (observed); `page` reaches page 100 at least |
| DataCite | GET `/dois` | `query` (Elasticsearch query string) | all metadata fields | `sort=relevance` (the API's default returns newest updated first) | 1,000 per page, 10,000 deep |
| OpenAIRE Graph | GET `/graph/v3/research-products` | `search` | title, abstract, subjects | relevance | 100 per page, 10,000 deep; `cursor` beyond |
| Google Dataset Search (opt-in) | GET `/search` (HTML) | `query` | Google's | Google's | 20 (observed; `start`, `page` and `offset` are ignored, 2026-10-06) |
| Zenodo (InvenioRDM) | GET `/api/records` | `q` (Elasticsearch syntax) | all fields | `bestmatch` | 25 anonymous, 100 with token; `page` reaches 10,000 results |
| Figshare | POST `/v2/articles/search` | `search_for` in a JSON body | all fields, field operators like `:title:` | `created_date`, newest first (no relevance order) | 1,000; offset capped at 1,000 |
| Dataverse | GET `/api/search` | `q` (Solr syntax) | all fields | Solr relevance | 1,000 |
| CKAN | GET `/api/3/action/package_search` | `q` (Solr syntax), `fq=dataset_type:dataset` | all fields | Solr relevance | 1,000 (a site may lower it); all six registered portals served 100 |
| data.europa.eu | GET `/api/hub/search/search` | `q`, `filters=dataset`, `includes`, `aggregation=false` | stemmed, multilingual | relevance | 1,000 |
| Data.gov | GET `/search` or `api.gsa.gov/.../v4/search` | `q` | OpenSearch full text | relevance; cursor `after` | 1,000 (`per_page`, OpenAPI) |
| Socrata Discovery | GET `/api/catalog/v1` | `q`, `only=dataset` | name, description, category, tags, column names and descriptions, attribution; with 3 terms or fewer all must match, otherwise 60% | relevance | 10,000 per request (`limit`); `offset + limit` <= 10,000, `scroll_id` beyond |
| OpenDataSoft | GET `/api/explore/v2.1/catalog/datasets` | `where=search("...")` (ODSQL) | fuzzy; the last term is a prefix | relevance | 100 |
| ArcGIS Hub | GET `/api/search/v1/collections/dataset/items` | `q` (OGC API Records) | title, tags, description | relevance | `numberMatched` caps at 10,000 (observed) |
| DBnomics | GET `/v22/search` | `q` | dataset names and codes | relevance | 100 (`limit` above 100 answers 400) |
| NADA | GET `/api/catalog/search` | `sk` | any word, loose | relevance | no cap seen up to 1,000 (`ps`) |
| NASA CMR | GET `/search/collections.umm_json` | `keyword` | all collection metadata | relevance | 2,000 |
| NOAA NCEI | GET `/access/services/search/v1/datasets` | `text` | name and description, any word | relevance | not documented; the index holds 100 datasets (observed) |
| PANGAEA | GET `/advanced/search.php` (internal, undocumented) | `q` (PANGAEA syntax, words ANDed) | full text | score per hit | 500; `offset` to 9,999 (observed) |
| GBIF | GET `/v1/dataset/search` | `q` | full text | relevance | 1,000 |
| DataONE | GET `/cn/v2/query/solr/` | Solr `q`, `fq` | Solr fields | Solr relevance | 10,000 (observed) |
| NCBI GEO | GET `esearch.fcgi` then `esummary.fcgi` | `term` (Entrez syntax) | indexed fields, `[ETYP]` filters | Entrez default order, not an explicit relevance rank | 10,000 (`retmax`); `retstart` pages |
| EBI Search | GET `/ebisearch/ws/rest/{domain}` | `query` | domain fields | relevance | 1,000 |
| OmicsDI | GET `/ws/dataset/search` | `query` (plus `NOT repository:"biostudies-literature"`) | full text | default order (docs: relevance) | 100 (docs; the server answers more) |
| Synapse | POST `/repo/v1/search` | `queryTerm` list, `booleanQuery` | any word, ranked | relevance | 1,000 per page (observed) |
| DANDI | GET `/api/dandisets/` | `search`, `ordering`, `empty=false` | every word as a substring of the version metadata, ignoring case; a `key:value` word is a filter | none; oldest first, `ordering=-stars` sorts by stars | 1,000 |
| CERN Open Data | GET `/api/records/` | `q`, `type=Dataset` | Invenio | relevance | 10,000 results (observed, HTTP 400 beyond) |
| CESSDA | GET `/api/DataSets/v2/search` | `q`, mandatory `metadataLanguage` | full text, every word must match | relevance | 200 per request; `offset + limit` <= 10,000 |
| Mendeley Data (opt-in) | GET `/api/research-data/search` | `query` | full text | relevance | 500 (`page_size`); `page` x `page_size` at most 10,000 |
| OSF via SHARE | GET `/trove/index-card-search` | `cardSearchText`, IRI-valued `cardSearchFilter` | full text | relevance | 100 verified, October 2026 |
| Data Commons | GET `/v2/resolve` | `nodes`, `resolver=indicator` | semantic match to statistical variables | similarity score | all matches in one response, no paging (docs) |
| Our World in Data | GET `/api/search` | `q`, `hitsPerPage` | charts and explorer views (`type=charts`; `type=pages` is articles) | relevance | 100 |
| FRED | GET `/fred/series/search` | `search_text` | full text | relevance or popularity | 1,000 |
| GitHub | GET `/search/repositories` | `q` with `topic:dataset` | name, description, topics | best match | 100; first 1,000 results only (page 11 answers 422) |
| ModelScope | GET `/api/v1/dolphin/datasets` | `Query` | names, authors and descriptions, every word must match | site default | 100 per page, larger values capped (October 2026) |
| Roboflow Universe | GET `/universe/search` | `q`, inline filters (`images>100`) | names, classes | site default | 12 a page by the SDK default; `page` from 1 |

## Catalogs searched locally

These publish a complete list but no search endpoint, so dataseek downloads the list (weekly) and ranks it itself (`src/catalog.rs`: every word must match a token prefix; title matches count three times; the whole query in the title wins).

| source | list endpoint | entries (2026-10-06) |
|---|---|---|
| OpenML | `/api/v1/json/data/list/limit/20000/status/active` (latest version per name) | 5,025 |
| UCI | `/api/datasets/list` | 689 |
| World Bank indicators | `/v2/indicator?per_page=40000` | 29,533 |
| SDMX agencies | `/dataflow` as SDMX-ML 2.1 | IMF 222 (101 with a portal page), OECD 1,548, ECB 215, BIS 32, ILO 1,216, UNdata 15, Bundesbank 94 |
| Eurostat | catalogue table of contents (`toc/txt`) | 7,563 |
| WHO GHO | OData `/api/Indicator` | 3,099 |
| STAC catalogs | `/collections?limit=1000`, following `next` | Planetary Computer 138, Earth Search 9, Copernicus Data Space 427, CDS 144 |
| Earth Engine | catalog HTML page | 867 |
| AWS Open Data | registry index HTML page | 1,215 |
| TensorFlow Datasets | catalog overview page: names and category headings | 446 |
| CELLxGENE | Curation API `/collections` | 397 |
| OpenNeuro | GraphQL `datasets`, cursor pages of 100 | 1,905 |
| PhysioNet | `/api/v1/projects/published/` | 477 |
| MPContribs | `/projects/` | 72 |
| NOMAD | `/datasets/`, `page_after_value` paging | 2,121 |
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
| OpenAIRE | terms: 60 per hour unauthenticated, 7,200 authenticated; 7,199 per hour observed | higher with a personal token |
| api.data.gov | `DEMO_KEY`: 30 per hour, 50 per day | 1,000 per hour |
| NCBI E-utilities | 3 per second, with `tool` and `email` | 10 per second |
| GitHub search | 10 per minute | 30 per minute |
| CERN Open Data | 60 per minute, 429 with `Retry-After: 60` (headers) | none |
| OpenDataSoft | 10,000 calls a day, shared by every anonymous caller | none |
| OSF (trove) | unstated; 17 requests in 3 minutes drew an hour-long 429 | none |
| OpenAlex (not used) | $0.10 per day of search credit | $1 per day free, then paid |

## Quirks that shaped the code

- The CKAN API answers 404 on catalog.data.gov (checked 2026-10-06); the documented Catalog API and catalog.data.gov's own `/search` return the same JSON, and catalog.data.gov/openapi.json describes the latter.
- `cn.dataone.org` renegotiates TLS on `/cn/` paths to ask for an optional client certificate, which rustls refuses; `search.dataone.org` serves the same index.
- OSF has no dataset resource type; projects and registrations are searched, and SHARE filters take IRIs, not bare names. Owners may set `resourceNature` to Dataset (4,135 of more than 10,000 cards), too few to filter on.
- OpenNeuro's GraphQL `search` returns nothing anonymously; the full list is paged instead.
- DataCite's dataset type is dominated by machine events: GBIF mints a DOI per download (4.73 million), CCDC per crystal structure (1.26 million). Both clients are dropped.
- Papers with Code's API redirects to Hugging Face trending papers since July 2025.
