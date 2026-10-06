//! STAC API collection catalogs. STAC has no cross-catalog search, and most
//! APIs lack the collection-search extension, so each catalog's collection
//! list is downloaded (following `next` links) and searched locally.
//!
//! - Planetary Computer takes at most `limit=1000` ("Collection limit must be
//!   between 1 and 1000"); Copernicus Data Space lists 10 a page by default
//!   and answers all of its collections to the same request (probes,
//!   October 2026).
//! - Copernicus Data Space limits its STAC catalogue to 5 requests a second
//!   (Copernicus Data Space forum, March 2026); a download is one request.
//! - Only Copernicus Data Space declares the collection-search free-text
//!   extension (its `conformsTo`, October 2026).
//! - `sci:doi` is the DOI of the data (STAC scientific citation extension):
//!   Planetary Computer fills 27 of 138 collections, Earth Search 1 of 9,
//!   Copernicus Data Space 159 of 427 and the Climate Data Store 140 of 144;
//!   only the Climate Data Store sends `updated` (October 2026). Collections
//!   that share a `sci:doi`, such as one product's COG and NetCDF variants,
//!   merge into one hit.
//! - Copernicus Data Space documents its STAC browser as the page for a
//!   collection (Copernicus Data Space documentation, October 2026). Earth
//!   Search has no collection page, so its link stays the collection's JSON.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, from_markdown, items, text};

pub struct Catalog {
    pub collections: &'static str,
    /// The human page for a collection id, when the catalog has one; the
    /// collection's own JSON is used otherwise.
    pub page: Option<&'static str>,
    pub publisher: &'static str,
}

pub static PLANETARY_COMPUTER: Catalog = Catalog {
    collections: "https://planetarycomputer.microsoft.com/api/stac/v1/collections",
    page: Some("https://planetarycomputer.microsoft.com/dataset/"),
    publisher: "Microsoft Planetary Computer",
};
pub static EARTH_SEARCH: Catalog = Catalog {
    collections: "https://earth-search.aws.element84.com/v1/collections",
    page: None,
    publisher: "Element 84",
};
pub static COPERNICUS_DATASPACE: Catalog = Catalog {
    collections: "https://stac.dataspace.copernicus.eu/v1/collections",
    page: Some("https://browser.stac.dataspace.copernicus.eu/collections/"),
    publisher: "Copernicus Data Space Ecosystem",
};
pub static COPERNICUS_CDS: Catalog = Catalog {
    collections: "https://cds.climate.copernicus.eu/api/catalogue/v1/collections",
    page: Some("https://cds.climate.copernicus.eu/datasets/"),
    publisher: "Copernicus Climate Change Service",
};

pub fn list(
    ctx: &Ctx<'_>,
    catalog: &Catalog,
) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    let mut next = Some(format!("{}?limit=1000", catalog.collections));
    for _ in 0..50 {
        let Some(url) = next.take() else { break };
        let body = ctx.http.get(&url).slow().json()?;
        let (page, after) = parse(catalog, &body)?;
        entries.extend(page);
        next = after;
    }
    Ok(entries)
}

/// One page of collections and the link to the next page, if any.
pub(super) fn parse(
    catalog: &Catalog,
    body: &Value,
) -> Result<(Vec<Dataset>, Option<String>), SourceError> {
    if body.get("collections").is_none() {
        return Err(SourceError::shape("no collections array"));
    }
    let entries = items(body, "/collections")
        .iter()
        .filter_map(|c| record(catalog, c))
        .collect();
    let next = items(body, "/links").iter().find_map(|link| {
        (link.get("rel").and_then(Value::as_str) == Some("next"))
            .then(|| text(link, "/href"))
            .flatten()
    });
    Ok((entries, next))
}

fn record(catalog: &Catalog, collection: &Value) -> Option<Dataset> {
    let id = text(collection, "/id")?;
    let url = match catalog.page {
        Some(prefix) => format!("{prefix}{id}"),
        None => format!("{}/{id}", catalog.collections),
    };
    let title = text(collection, "/title").unwrap_or_else(|| id.clone());
    let mut dataset = Dataset::new(&title, &url)
        .describe(text(collection, "/description").map(|d| from_markdown(&d)))
        .doi_from(text(collection, "/sci:doi"));
    dataset.publisher = Some(catalog.publisher.to_owned());
    dataset.license = text(collection, "/license")
        .filter(|l| l != "proprietary" && l != "other" && l != "various");
    dataset.updated = day(text(collection, "/updated"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let (entries, next) =
            parse(&EARTH_SEARCH, &fixture::json("stac.json")).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(next, None);
        assert_eq!(
            entries[0],
            Dataset {
                title: "Sentinel-2 Pre-Collection 1 Level-2A".into(),
                url: "https://earth-search.aws.element84.com/v1/collections/\
                      sentinel-2-pre-c1-l2a"
                    .into(),
                description: Some(
                    "Sentinel-2 Pre-Collection 1 Level-2A (baseline < 05.00), \
                     with data and metadata matching collection \
                     sentinel-2-c1-l2a"
                        .into()
                ),
                publisher: Some("Element 84".into()),
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(
            entries[2].description.as_deref(),
            Some(
                "The National Agriculture Imagery Program (NAIP) provides \
                 U.S.-wide, high-resolution aerial imagery, with four \
                 spectral bands (R, G, B, IR)."
            ),
            "STAC descriptions are CommonMark"
        );
        assert_eq!(entries[2].doi.as_deref(), Some("10.5066/f7qn651g"));
        assert_eq!(entries[1].doi, None);
    }

    #[test]
    fn a_data_space_collection_links_to_its_browser_page() {
        let (entries, next) = parse(
            &COPERNICUS_DATASPACE,
            &fixture::json("stac.dataspace.json"),
        )
        .unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            next.as_deref(),
            Some(
                "https://stac.dataspace.copernicus.eu/v1/collections?limit=3&offset=12"
            )
        );
        assert_eq!(
            entries[0],
            Dataset {
                title: "CLMS Burnt Area (BA) Global 300m daily V3 (COG)"
                    .into(),
                url: "https://browser.stac.dataspace.copernicus.eu/\
                      collections/clms_ba_global_300m_daily_v3_cog"
                    .into(),
                description: Some(
                    "Maps burn scars, surfaces which have been sufficiently \
                     affected by fire to display significant changes in the \
                     vegetation cover (destruction of dry material, \
                     reduction or loss of green material) and in the ground \
                     surface (temporarily darker because of ash). Daily \
                     datasets are available at global scale, in the spatial..."
                        .into()
                ),
                publisher: Some("Copernicus Data Space Ecosystem".into()),
                doi: Some(
                    "10.2909/9c0519f9-d2c2-4469-a9e1-2222d37c33d6".into()
                ),
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(entries[0].doi, entries[1].doi, "one product, two formats");
        assert_ne!(entries[0].doi, entries[2].doi);
    }

    #[test]
    fn the_climate_data_store_sends_a_doi_a_license_and_an_update_date() {
        let (entries, next) =
            parse(&COPERNICUS_CDS, &fixture::json("stac.cds.json")).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(next, None);
        let era5 = &entries[1];
        assert_eq!(
            era5.title,
            "ERA5 post-processed daily statistics on single levels from \
             1940 to present"
        );
        assert_eq!(
            era5.url,
            "https://cds.climate.copernicus.eu/datasets/\
             derived-era5-single-levels-daily-statistics"
        );
        assert_eq!(era5.doi.as_deref(), Some("10.24381/cds.4991cf48"));
        assert_eq!(era5.license.as_deref(), Some("CC-BY-4.0"));
        assert_eq!(era5.updated.as_deref(), Some("2026-10-06"));
        assert_eq!(entries[0].license, None, "\"other\" is not a license");
        assert_eq!(entries[0].doi.as_deref(), Some("10.24381/cds.c14d9324"));
    }
}
