//! The registry of every dataset source dataseek searches, and the dispatch
//! from a registry entry to its adapter.
//!
//! A source is one row in [`SOURCES`]: an id users type (`--source zenodo`),
//! what it holds, the shared protocol it speaks, where its API is documented,
//! which key it needs, and its [`Adapter`]. Protocol adapters (CKAN,
//! Dataverse, NADA, Socrata, STAC, SDMX, EBI Search) are written once and
//! configured per row; everything else has its own module under `sources/`.
//!
//! Adapters come in two kinds. Live adapters send the query to the source and
//! return its ranking. Catalog adapters download the source's whole list
//! (cached for [`crate::cache::CATALOG_TTL`]) and search it locally through
//! [`crate::catalog`], because the source has no search endpoint. Rules for
//! every adapter: return at most `limit` records in the source's own order,
//! drop records without a title or link ([`Dataset::valid`]), map a changed
//! response to [`SourceError::Shape`] rather than an empty list, and never put
//! a credential into a URL or message that could be printed.

mod arcgis;
mod aws;
mod cellxgene;
mod census;
mod cern;
mod cessda;
mod ckan;
mod cmr;
mod dandi;
mod datacite;
mod datacommons;
mod datagov;
mod dataone;
mod dataverse;
mod dbnomics;
mod ebi;
mod europa;
mod eurostat;
mod figshare;
mod fiscal;
mod fred;
mod gbif;
mod gee;
mod github;
mod google;
mod huggingface;
mod kaggle;
mod materials;
mod mendeley;
mod modelscope;
mod nada;
mod ncbi;
mod ncei;
mod nomad;
mod omicsdi;
mod openaire;
mod opendatasoft;
mod openml;
mod openneuro;
mod osf;
mod owid;
mod pangaea;
mod physionet;
mod roboflow;
mod sdmx;
mod socrata;
mod stac;
mod synapse;
mod tfds;
mod uci;
mod who;
mod worldbank;
mod zenodo;

use std::fmt;

use crate::cache::{CATALOG_TTL, Cache, Freshness, Kind};
use crate::credentials::{Credentials, Key};
use crate::http::{Http, SourceError};
use crate::record::Dataset;

pub type Live = fn(&Ctx<'_>, &str, usize) -> Result<Vec<Dataset>, SourceError>;
pub type Listing = fn(&Ctx<'_>) -> Result<Vec<Dataset>, SourceError>;

/// What an adapter gets to work with.
pub struct Ctx<'a> {
    pub http: &'a Http,
    pub creds: &'a Credentials,
    pub cache: &'a Cache,
    pub refresh: bool,
}

pub enum Adapter {
    Live(Live),
    Catalog(Listing),
    Ckan(&'static ckan::Portal),
    Dataverse(&'static str),
    Nada(&'static str),
    Socrata(&'static str),
    Ebi(&'static ebi::Domain),
    Stac(&'static stac::Catalog),
    Sdmx(&'static sdmx::Agency),
}

/// What a source mainly holds. The names are what `--category` accepts and
/// what `sources` prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Category {
    /// Search engines and DOI registries spanning many repositories.
    Aggregator,
    /// Machine learning hubs and benchmark collections.
    MachineLearning,
    /// General research data repositories.
    Research,
    /// Government open-data portals.
    Government,
    /// Official and general statistics.
    Statistics,
    /// Macroeconomic and development data: IMF, OECD, World Bank, ILO.
    Economics,
    /// Central bank, market and public-finance data.
    Finance,
    /// Earth observation, climate and geospatial catalogs.
    Geospatial,
    /// Genomics, proteomics and biomedical archives.
    LifeSciences,
    /// Neuroimaging, neurophysiology and clinical signals.
    Neuroscience,
    /// Biodiversity and environmental science.
    Ecology,
    /// Survey and social science archives.
    SocialScience,
    /// Particle physics and materials science.
    Physics,
    /// Code hosting.
    Code,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use clap::ValueEnum;
        match self.to_possible_value() {
            Some(value) => f.write_str(value.get_name()),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// The source refuses requests without the key; it is skipped otherwise.
    Required,
    /// The key raises a rate limit or unlocks a better endpoint.
    Optional,
}

pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub protocol: &'static str,
    pub docs: &'static str,
    pub key: Option<(Key, Need)>,
    /// False where the terms forbid storing results (Kaggle).
    pub persist: bool,
    pub adapter: Adapter,
}

impl Source {
    pub fn is_catalog(&self) -> bool {
        matches!(
            self.adapter,
            Adapter::Catalog(_) | Adapter::Stac(_) | Adapter::Sdmx(_)
        )
    }

    /// The key this source cannot run without, when it is missing.
    pub fn missing_key(&self, creds: &Credentials) -> Option<Key> {
        match self.key {
            Some((key, Need::Required)) if creds.get(key).is_none() => {
                Some(key)
            }
            _ => None,
        }
    }

    pub fn search(
        &self,
        ctx: &Ctx<'_>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Dataset>, SourceError> {
        match &self.adapter {
            Adapter::Live(run) => run(ctx, query, limit),
            Adapter::Ckan(portal) => ckan::search(ctx, portal, query, limit),
            Adapter::Dataverse(base) => {
                dataverse::search(ctx, base, query, limit)
            }
            Adapter::Nada(base) => nada::search(ctx, base, query, limit),
            Adapter::Socrata(base) => socrata::search(ctx, base, query, limit),
            Adapter::Ebi(domain) => ebi::search(ctx, domain, query, limit),
            Adapter::Catalog(list) => {
                self.local(ctx, query, limit, || list(ctx))
            }
            Adapter::Stac(catalog) => {
                self.local(ctx, query, limit, || stac::list(ctx, catalog))
            }
            Adapter::Sdmx(agency) => {
                self.local(ctx, query, limit, || sdmx::list(ctx, agency))
            }
        }
    }

    /// Download and cache this source's catalog now; `None` for live
    /// sources, which have no catalog.
    pub fn warm(&self, ctx: &Ctx<'_>) -> Option<Result<usize, SourceError>> {
        let downloaded = match &self.adapter {
            Adapter::Catalog(list) => list(ctx),
            Adapter::Stac(catalog) => stac::list(ctx, catalog),
            Adapter::Sdmx(agency) => sdmx::list(ctx, agency),
            _ => return None,
        };
        Some(downloaded.map(|entries| {
            ctx.cache.store(Kind::Catalog, self.id, &entries);
            entries.len()
        }))
    }

    /// Search the cached catalog, downloading it when it is missing or
    /// expired. A failed download falls back to an expired copy. `--refresh`
    /// does not apply: catalogs have their own TTL and `cache warm`.
    fn local(
        &self,
        ctx: &Ctx<'_>,
        query: &str,
        limit: usize,
        download: impl FnOnce() -> Result<Vec<Dataset>, SourceError>,
    ) -> Result<Vec<Dataset>, SourceError> {
        let cached = ctx.cache.load::<Vec<Dataset>>(
            Kind::Catalog,
            self.id,
            CATALOG_TTL,
        );
        let entries = match cached {
            Some((entries, Freshness::Fresh)) => entries,
            stale => match download() {
                Ok(entries) if !entries.is_empty() => {
                    ctx.cache.store(Kind::Catalog, self.id, &entries);
                    entries
                }
                Ok(_) => {
                    return Err(SourceError::shape(
                        "the catalog came back empty",
                    ));
                }
                Err(error) => match stale {
                    Some((entries, _)) => entries,
                    None => return Err(error),
                },
            },
        };
        Ok(crate::catalog::search(&entries, query, limit))
    }
}

/// The registry rows to ask: the named ones (or all), narrowed to the given
/// categories, minus exclusions, in registry order. Ids were validated by
/// clap.
pub fn select(
    only: &[String],
    exclude: &[String],
    categories: &[Category],
) -> Vec<&'static Source> {
    SOURCES
        .iter()
        .filter(|s| only.is_empty() || only.iter().any(|id| id == s.id))
        .filter(|s| categories.is_empty() || categories.contains(&s.category))
        .filter(|s| !exclude.iter().any(|id| id == s.id))
        .collect()
}

/// What every command that talks to sources needs, built once per run.
pub struct Services {
    pub http: Http,
    pub creds: Credentials,
    pub cache: Cache,
}

impl Services {
    pub fn load() -> anyhow::Result<Self> {
        let dirs = crate::paths::resolve()?;
        Ok(Self {
            http: Http::new(),
            creds: Credentials::load(&dirs.config),
            cache: Cache::new(dirs.cache),
        })
    }

    pub fn ctx(&self, refresh: bool) -> Ctx<'_> {
        Ctx {
            http: &self.http,
            creds: &self.creds,
            cache: &self.cache,
            refresh,
        }
    }
}

const fn live(
    id: &'static str,
    name: &'static str,
    category: Category,
    protocol: &'static str,
    docs: &'static str,
    run: Live,
) -> Source {
    Source {
        id,
        name,
        category,
        protocol,
        docs,
        key: None,
        persist: true,
        adapter: Adapter::Live(run),
    }
}

const fn listed(
    id: &'static str,
    name: &'static str,
    category: Category,
    protocol: &'static str,
    docs: &'static str,
    list: Listing,
) -> Source {
    Source {
        id,
        name,
        category,
        protocol,
        docs,
        key: None,
        persist: true,
        adapter: Adapter::Catalog(list),
    }
}

const fn keyed(mut source: Source, key: Key, need: Need) -> Source {
    source.key = Some((key, need));
    source
}

const fn via(
    id: &'static str,
    name: &'static str,
    category: Category,
    protocol: &'static str,
    docs: &'static str,
    adapter: Adapter,
) -> Source {
    Source {
        id,
        name,
        category,
        protocol,
        docs,
        key: None,
        persist: true,
        adapter,
    }
}

use Category::{
    Aggregator, Code, Ecology, Economics, Finance, Geospatial, Government,
    LifeSciences, MachineLearning, Neuroscience, Physics, Research,
    SocialScience, Statistics,
};

const CKAN_DOCS: &str = "https://docs.ckan.org/en/latest/api/";
const DATAVERSE_DOCS: &str =
    "https://guides.dataverse.org/en/latest/api/search.html";
const NADA_DOCS: &str =
    "https://microdata.worldbank.org/api-documentation/catalog/index.html";
const SOCRATA_DOCS: &str = "https://dev.socrata.com/docs/other/discovery";
const EBI_DOCS: &str =
    "https://www.ebi.ac.uk/ebisearch/documentation/rest-api";

pub static SOURCES: &[Source] = &[
    // Aggregators and general search engines.
    live(
        "datacite",
        "DataCite",
        Aggregator,
        "DataCite REST",
        "https://support.datacite.org/docs/api",
        datacite::search,
    ),
    live(
        "openaire",
        "OpenAIRE Graph",
        Aggregator,
        "OpenAIRE Graph",
        "https://graph.openaire.eu/docs/apis/graph-api/",
        openaire::search,
    ),
    live(
        "google",
        "Google Dataset Search",
        Aggregator,
        "results page data",
        "https://datasetsearch.research.google.com/help",
        google::search,
    ),
    via(
        "b2find",
        "EUDAT B2FIND",
        Aggregator,
        "CKAN",
        CKAN_DOCS,
        Adapter::Ckan(&ckan::B2FIND),
    ),
    // Machine learning.
    keyed(
        live(
            "huggingface",
            "Hugging Face Hub",
            MachineLearning,
            "Hub API",
            "https://huggingface.co/docs/hub/api",
            huggingface::search,
        ),
        Key::HuggingFace,
        Need::Optional,
    ),
    Source {
        persist: false,
        ..keyed(
            live(
                "kaggle",
                "Kaggle",
                MachineLearning,
                "Kaggle API",
                "https://www.kaggle.com/docs/api",
                kaggle::search,
            ),
            Key::Kaggle,
            Need::Optional,
        )
    },
    listed(
        "openml",
        "OpenML",
        MachineLearning,
        "OpenML REST, listed",
        "https://docs.openml.org/ecosystem/Rest/",
        openml::list,
    ),
    listed(
        "uci",
        "UCI Machine Learning Repository",
        MachineLearning,
        "list endpoint",
        "https://github.com/uci-ml-repo/ucimlrepo",
        uci::list,
    ),
    keyed(
        live(
            "roboflow",
            "Roboflow Universe",
            MachineLearning,
            "Universe API",
            "https://docs.roboflow.com/datasets/universe/universe/universe-search",
            roboflow::search,
        ),
        Key::Roboflow,
        Need::Required,
    ),
    live(
        "modelscope",
        "ModelScope",
        MachineLearning,
        "site API",
        "https://www.modelscope.cn/docs",
        modelscope::search,
    ),
    listed(
        "aws",
        "Registry of Open Data on AWS",
        MachineLearning,
        "registry page",
        "https://github.com/awslabs/open-data-registry",
        aws::list,
    ),
    listed(
        "tfds",
        "TensorFlow Datasets",
        MachineLearning,
        "catalog page",
        "https://www.tensorflow.org/datasets/catalog/overview",
        tfds::list,
    ),
    keyed(
        live(
            "github",
            "GitHub (topic:dataset)",
            Code,
            "GitHub search",
            "https://docs.github.com/en/rest/search/search",
            github::search,
        ),
        Key::GitHub,
        Need::Optional,
    ),
    // Research repositories.
    live(
        "zenodo",
        "Zenodo",
        Research,
        "InvenioRDM",
        "https://developers.zenodo.org/",
        zenodo::search,
    ),
    live(
        "figshare",
        "Figshare",
        Research,
        "Figshare",
        "https://docs.figshare.com/",
        figshare::search,
    ),
    via(
        "harvard-dataverse",
        "Harvard Dataverse",
        Research,
        "Dataverse",
        DATAVERSE_DOCS,
        Adapter::Dataverse("https://dataverse.harvard.edu"),
    ),
    via(
        "borealis",
        "Borealis (Canada)",
        Research,
        "Dataverse",
        DATAVERSE_DOCS,
        Adapter::Dataverse("https://borealisdata.ca"),
    ),
    via(
        "recherche-data-gouv",
        "Recherche Data Gouv (France)",
        Research,
        "Dataverse",
        DATAVERSE_DOCS,
        Adapter::Dataverse("https://entrepot.recherche.data.gouv.fr"),
    ),
    via(
        "dataverse-nl",
        "DataverseNL",
        Research,
        "Dataverse",
        DATAVERSE_DOCS,
        Adapter::Dataverse("https://dataverse.nl"),
    ),
    via(
        "dataverse-no",
        "DataverseNO",
        Research,
        "Dataverse",
        DATAVERSE_DOCS,
        Adapter::Dataverse("https://dataverse.no"),
    ),
    live(
        "osf",
        "OSF (via SHARE)",
        Research,
        "SHARE trove",
        "https://share.osf.io/trove/docs",
        osf::search,
    ),
    live(
        "mendeley",
        "Mendeley Data",
        Research,
        "site search API",
        "https://data.mendeley.com/api/docs/",
        mendeley::search,
    ),
    // Government open data.
    live(
        "europa",
        "data.europa.eu",
        Government,
        "DCAT-AP (piveau)",
        "https://dataeuropa.gitlab.io/data-provider-manual/api-documentation/",
        europa::search,
    ),
    keyed(
        live(
            "datagov",
            "Data.gov",
            Government,
            "Data.gov Catalog API",
            "https://resources.data.gov/catalog-api/",
            datagov::search,
        ),
        Key::DataGov,
        Need::Optional,
    ),
    via(
        "data-gov-uk",
        "data.gov.uk",
        Government,
        "CKAN",
        CKAN_DOCS,
        Adapter::Ckan(&ckan::DATA_GOV_UK),
    ),
    via(
        "open-canada",
        "Open Government Canada",
        Government,
        "CKAN",
        CKAN_DOCS,
        Adapter::Ckan(&ckan::OPEN_CANADA),
    ),
    via(
        "data-gov-au",
        "data.gov.au",
        Government,
        "CKAN",
        CKAN_DOCS,
        Adapter::Ckan(&ckan::DATA_GOV_AU),
    ),
    via(
        "govdata",
        "GovData (Germany)",
        Government,
        "CKAN",
        CKAN_DOCS,
        Adapter::Ckan(&ckan::GOVDATA),
    ),
    via(
        "hdx",
        "Humanitarian Data Exchange",
        Government,
        "CKAN",
        "https://data.humdata.org/faqs/devs",
        Adapter::Ckan(&ckan::HDX),
    ),
    via(
        "socrata",
        "Socrata portals (US)",
        Government,
        "Socrata Discovery",
        SOCRATA_DOCS,
        Adapter::Socrata("https://api.us.socrata.com"),
    ),
    via(
        "socrata-eu",
        "Socrata portals (EU)",
        Government,
        "Socrata Discovery",
        SOCRATA_DOCS,
        Adapter::Socrata("https://api.eu.socrata.com"),
    ),
    live(
        "opendatasoft",
        "OpenDataSoft hub",
        Government,
        "OpenDataSoft Explore",
        "https://help.opendatasoft.com/apis/ods-explore-v2/",
        opendatasoft::search,
    ),
    live(
        "arcgis",
        "ArcGIS Hub",
        Government,
        "OGC API Records",
        "https://hub.arcgis.com/api/search/v1",
        arcgis::search,
    ),
    // Statistics.
    live(
        "dbnomics",
        "DBnomics",
        Economics,
        "DBnomics",
        "https://api.db.nomics.world/v22/apidocs",
        dbnomics::search,
    ),
    listed(
        "worldbank",
        "World Bank indicators",
        Economics,
        "World Bank API, listed",
        "https://datahelpdesk.worldbank.org/knowledgebase/articles/889392",
        worldbank::list,
    ),
    via(
        "worldbank-microdata",
        "World Bank Microdata Library",
        Economics,
        "NADA",
        NADA_DOCS,
        Adapter::Nada("https://microdata.worldbank.org/index.php"),
    ),
    via(
        "ihsn",
        "IHSN survey catalog",
        Statistics,
        "NADA",
        NADA_DOCS,
        Adapter::Nada("https://catalog.ihsn.org/index.php"),
    ),
    via(
        "fao-microdata",
        "FAO Microdata",
        Statistics,
        "NADA",
        NADA_DOCS,
        Adapter::Nada("https://microdata.fao.org/index.php"),
    ),
    via(
        "unhcr-microdata",
        "UNHCR Microdata Library",
        Statistics,
        "NADA",
        NADA_DOCS,
        Adapter::Nada("https://microdata.unhcr.org/index.php"),
    ),
    via(
        "imf",
        "IMF",
        Economics,
        "SDMX",
        "https://portal.api.imf.org/",
        Adapter::Sdmx(&sdmx::IMF),
    ),
    via(
        "oecd",
        "OECD",
        Economics,
        "SDMX",
        "https://sdmx.oecd.org/public/rest/",
        Adapter::Sdmx(&sdmx::OECD),
    ),
    via(
        "ecb",
        "European Central Bank",
        Finance,
        "SDMX",
        "https://data.ecb.europa.eu/help/api/overview",
        Adapter::Sdmx(&sdmx::ECB),
    ),
    listed(
        "eurostat",
        "Eurostat",
        Statistics,
        "Eurostat table of contents",
        "https://ec.europa.eu/eurostat/web/user-guides/data-browser/api-data-access",
        eurostat::list,
    ),
    via(
        "bis",
        "Bank for International Settlements",
        Finance,
        "SDMX",
        "https://stats.bis.org/api-doc/v2/",
        Adapter::Sdmx(&sdmx::BIS),
    ),
    via(
        "ilo",
        "ILOSTAT",
        Economics,
        "SDMX",
        "https://ilostat.ilo.org/resources/sdmx-tools/",
        Adapter::Sdmx(&sdmx::ILO),
    ),
    via(
        "undata",
        "UNdata",
        Statistics,
        "SDMX",
        "https://data.un.org/Host.aspx?Content=API",
        Adapter::Sdmx(&sdmx::UNDATA),
    ),
    keyed(
        live(
            "datacommons",
            "Data Commons",
            Statistics,
            "Data Commons REST v2",
            "https://docs.datacommons.org/api/rest/v2/",
            datacommons::search,
        ),
        Key::DataCommons,
        Need::Optional,
    ),
    live(
        "owid",
        "Our World in Data",
        Statistics,
        "site search API",
        "https://docs.owid.io/projects/etl/api/",
        owid::search,
    ),
    keyed(
        live(
            "fred",
            "FRED",
            Finance,
            "FRED API",
            "https://fred.stlouisfed.org/docs/api/fred/series_search.html",
            fred::search,
        ),
        Key::Fred,
        Need::Required,
    ),
    via(
        "bundesbank",
        "Deutsche Bundesbank",
        Finance,
        "SDMX",
        "https://www.bundesbank.de/en/statistics/time-series-databases/help-for-sdmx-web-service",
        Adapter::Sdmx(&sdmx::BUNDESBANK),
    ),
    listed(
        "fiscal-data",
        "U.S. Treasury Fiscal Data",
        Finance,
        "Fiscal Data API, listed",
        "https://fiscaldata.treasury.gov/api-documentation/",
        fiscal::list,
    ),
    listed(
        "census",
        "U.S. Census Bureau API",
        Statistics,
        "DCAT data.json, listed",
        "https://www.census.gov/data/developers/guidance/api-user-guide.html",
        census::list,
    ),
    listed(
        "who",
        "WHO Global Health Observatory",
        Statistics,
        "OData, listed",
        "https://www.who.int/data/gho/info/gho-odata-api",
        who::list,
    ),
    // Earth observation and geospatial.
    live(
        "cmr",
        "NASA Earthdata (CMR)",
        Geospatial,
        "CMR search",
        "https://cmr.earthdata.nasa.gov/search/site/docs/search/api.html",
        cmr::search,
    ),
    via(
        "planetary-computer",
        "Microsoft Planetary Computer",
        Geospatial,
        "STAC",
        "https://planetarycomputer.microsoft.com/docs/reference/stac/",
        Adapter::Stac(&stac::PLANETARY_COMPUTER),
    ),
    via(
        "earth-search",
        "Earth Search (AWS)",
        Geospatial,
        "STAC",
        "https://element84.com/earth-search/",
        Adapter::Stac(&stac::EARTH_SEARCH),
    ),
    via(
        "copernicus-dataspace",
        "Copernicus Data Space",
        Geospatial,
        "STAC",
        "https://documentation.dataspace.copernicus.eu/APIs/STAC.html",
        Adapter::Stac(&stac::COPERNICUS_DATASPACE),
    ),
    via(
        "copernicus-cds",
        "Copernicus Climate Data Store",
        Geospatial,
        "STAC",
        "https://cds.climate.copernicus.eu/how-to-api",
        Adapter::Stac(&stac::COPERNICUS_CDS),
    ),
    listed(
        "earth-engine",
        "Google Earth Engine catalog",
        Geospatial,
        "catalog page",
        "https://developers.google.com/earth-engine/datasets/catalog",
        gee::list,
    ),
    live(
        "ncei",
        "NOAA NCEI",
        Geospatial,
        "NCEI Search Service",
        "https://www.ncei.noaa.gov/support/access-search-service-api-user-documentation",
        ncei::search,
    ),
    live(
        "pangaea",
        "PANGAEA",
        Geospatial,
        "PANGAEA search",
        "https://wiki.pangaea.de/wiki/PANGAEA_search",
        pangaea::search,
    ),
    // Ecology.
    live(
        "gbif",
        "GBIF",
        Ecology,
        "GBIF registry",
        "https://techdocs.gbif.org/en/openapi/v1/registry",
        gbif::search,
    ),
    live(
        "dataone",
        "DataONE",
        Ecology,
        "DataONE Solr",
        "https://dataoneorg.github.io/api-documentation/",
        dataone::search,
    ),
    // Life sciences.
    keyed(
        live(
            "geo",
            "NCBI GEO",
            LifeSciences,
            "E-utilities",
            "https://www.ncbi.nlm.nih.gov/books/NBK25501/",
            ncbi::search,
        ),
        Key::Ncbi,
        Need::Optional,
    ),
    via(
        "arrayexpress",
        "ArrayExpress (EBI)",
        LifeSciences,
        "EBI Search",
        EBI_DOCS,
        Adapter::Ebi(&ebi::ARRAYEXPRESS),
    ),
    via(
        "biostudies",
        "BioStudies (EBI)",
        LifeSciences,
        "EBI Search",
        EBI_DOCS,
        Adapter::Ebi(&ebi::BIOSTUDIES),
    ),
    live(
        "omicsdi",
        "OmicsDI",
        LifeSciences,
        "OmicsDI",
        "https://www.omicsdi.org/ws/",
        omicsdi::search,
    ),
    listed(
        "cellxgene",
        "CZ CELLxGENE",
        LifeSciences,
        "Curation API, listed",
        "https://api.cellxgene.cziscience.com/curation/ui/",
        cellxgene::list,
    ),
    live(
        "synapse",
        "Synapse",
        LifeSciences,
        "Synapse REST",
        "https://rest-docs.synapse.org/",
        synapse::search,
    ),
    // Neuroscience and clinical.
    listed(
        "openneuro",
        "OpenNeuro",
        Neuroscience,
        "GraphQL, listed",
        "https://docs.openneuro.org/api.html",
        openneuro::list,
    ),
    live(
        "dandi",
        "DANDI Archive",
        Neuroscience,
        "DANDI REST",
        "https://api.dandiarchive.org/swagger/",
        dandi::search,
    ),
    listed(
        "physionet",
        "PhysioNet",
        Neuroscience,
        "project list",
        "https://physionet.org/about/",
        physionet::list,
    ),
    // Physics and materials.
    live(
        "cern",
        "CERN Open Data",
        Physics,
        "Invenio",
        "https://github.com/cernopendata/opendata.cern.ch",
        cern::search,
    ),
    listed(
        "materials-project",
        "Materials Project (MPContribs)",
        Physics,
        "MPContribs, listed",
        "https://api.materialsproject.org/docs",
        materials::list,
    ),
    listed(
        "nomad",
        "NOMAD",
        Physics,
        "NOMAD API, listed",
        "https://nomad-lab.eu/prod/v1/api/v1/extensions/docs",
        nomad::list,
    ),
    // Social science.
    live(
        "cessda",
        "CESSDA Data Catalogue",
        SocialScience,
        "CESSDA",
        "https://api.tech.cessda.eu/",
        cessda::search,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_lowercase_and_typeable() {
        let mut seen = std::collections::HashSet::new();
        for source in SOURCES {
            assert!(seen.insert(source.id), "duplicate id {}", source.id);
            assert!(
                source.id.chars().all(|c| c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '-'),
                "{} is not a plain lowercase id",
                source.id
            );
        }
    }

    #[test]
    fn every_source_links_its_documentation() {
        for source in SOURCES {
            assert!(source.docs.starts_with("https://"), "{}", source.id);
        }
    }

    #[test]
    fn kaggle_results_are_never_written_to_disk() {
        assert!(!SOURCES.iter().find(|s| s.id == "kaggle").unwrap().persist);
    }
}
