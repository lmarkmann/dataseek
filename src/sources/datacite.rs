//! DataCite DOI search, restricted to resource type Dataset. One adapter that
//! reaches the DOI-minting repositories at once (Zenodo, Figshare, Dryad,
//! Dataverse installations, ICPSR, UKDS, GESIS, Pangaea, ...).
//!
//! Two kinds of noise are handled here rather than left to the user: DOIs
//! minted per machine event (GBIF user downloads, CCDC crystal structures)
//! are dropped, and every version DOI carries its
//! concept DOI as an alias so `dedup` folds versions into one hit. The page
//! asks for twice the limit to leave room for what is dropped.

use serde_json::Value;

use super::Ctx;
use crate::http::{CONTACT, SourceError};
use crate::record::{Dataset, day, doi, items, text};

/// Dataset DOIs that are machine events, not datasets a person would look
/// for: one per CCDC crystal structure (1.26M) and one per GBIF user
/// download (most of gbif.gbif's 4.73M; its real datasets stay). Counts
/// measured 2026-10-06.
const NOISE_CLIENTS: &[&str] = &["ccdc.csd"];
const GBIF_DOWNLOADS: &str = "10.15468/dl.";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.datacite.org/dois")
        .query("query", query)
        .query("resource-type-id", "dataset")
        .query("page[size]", limit.saturating_mul(2).clamp(1, 100))
        .query("affiliation", "false")
        .query("mailto", CONTACT)
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("data").is_none() {
        return Err(SourceError::shape("no data array"));
    }
    Ok(items(body, "/data")
        .iter()
        .filter(|row| {
            let client = text(row, "/relationships/client/data/id");
            let id = text(row, "/attributes/doi").unwrap_or_default();
            client.is_none_or(|c| !NOISE_CLIENTS.contains(&c.as_str()))
                && !id.to_lowercase().starts_with(GBIF_DOWNLOADS)
        })
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let attributes = row.get("attributes")?;
    let id = text(attributes, "/doi")?;
    let url = text(attributes, "/url")
        .unwrap_or_else(|| format!("https://doi.org/{id}"));
    let title = text(attributes, "/titles/0/title")?;
    let abstract_text = items(attributes, "/descriptions")
        .iter()
        .find(|d| {
            d.get("descriptionType").and_then(Value::as_str)
                == Some("Abstract")
        })
        .or_else(|| items(attributes, "/descriptions").first())
        .and_then(|d| text(d, "/description"));
    let mut dataset =
        Dataset::new(&title, &url).describe(abstract_text).doi_from(Some(id));
    dataset.publisher = text(attributes, "/publisher")
        .or_else(|| text(attributes, "/publisher/name"));
    dataset.license = items(attributes, "/rightsList").iter().find_map(|r| {
        text(r, "/rightsIdentifier").or_else(|| text(r, "/rights"))
    });
    dataset.updated = day(text(attributes, "/updated"));
    dataset.aliases = items(attributes, "/relatedIdentifiers")
        .iter()
        .filter(|r| {
            r.get("relationType").and_then(Value::as_str)
                == Some("IsVersionOf")
        })
        .filter_map(|r| text(r, "/relatedIdentifier").as_deref().and_then(doi))
        .collect();
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("datacite.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Crop Wild Relatives (CWRs) of Bangladesh: An \
                        Integrated Database of Species Occurrence, \
                        Distribution, Habitat and Herbarium Records"
                    .into(),
                url: "https://www.gbif.org/dataset/\
                      c0eddfec-c87b-47c5-977a-dd2c7c7e5dec"
                    .into(),
                description: Some(
                    "This dataset compiles occurrence records of crop wild \
                     relatives (CWR) of cultivated crops in Bangladesh. \
                     Cultivated crops occurring in Bangladesh were \
                     identified and categorized based on information from \
                     the Food and Agriculture Organization (FAO) FAOSTAT \
                     database and Banglapedia."
                        .into()
                ),
                publisher: Some("Jagannath University".into()),
                doi: Some("10.15468/zrsahs".into()),
                license: Some("cc-by-4.0".into()),
                updated: Some("2026-10-06".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(hits[2].aliases, ["10.5281/zenodo.23111497"]);
    }
}
