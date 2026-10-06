//! STAC API collection catalogs. STAC has no cross-catalog search, and most
//! APIs lack the collection-search extension, so each catalog's collection
//! list is downloaded (following `next` links) and searched locally.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, from_markdown, items, text};

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
    page: None,
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
        .describe(text(collection, "/description").map(|d| from_markdown(&d)));
    dataset.publisher = Some(catalog.publisher.to_owned());
    dataset.license = text(collection, "/license")
        .filter(|l| l != "proprietary" && l != "other" && l != "various");
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
    }
}
