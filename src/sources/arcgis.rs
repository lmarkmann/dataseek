//! ArcGIS Hub's global search (OGC API Records) over public ArcGIS Online
//! items, limited to the `dataset` collection so maps and apps stay out.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, date_from_epoch, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://hub.arcgis.com/api/search/v1/collections/dataset/items")
        .query("q", query)
        .query("limit", limit)
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("features").is_none() {
        return Err(SourceError::shape("no features array"));
    }
    Ok(items(body, "/features")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(feature: &Value) -> Option<Dataset> {
    let props = feature.get("properties")?;
    let id = text(props, "/id")?;
    let mut dataset = Dataset::new(
        &text(props, "/title")?,
        &format!("https://hub.arcgis.com/datasets/{id}"),
    )
    .describe(text(props, "/description").or_else(|| text(props, "/snippet")));
    dataset.publisher =
        text(props, "/source").or_else(|| text(props, "/owner"));
    dataset.license = text(props, "/license")
        .filter(|l| !matches!(l.as_str(), "custom" | "none"));
    dataset.updated = number(props, "/modified").and_then(date_from_epoch);
    dataset.size_bytes = number(props, "/size");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("arcgis.json"), 10).unwrap();
        assert_eq!(hits.len(), 5);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Water".into(),
                url: "https://hub.arcgis.com/datasets/\
                      27f4373c24fc421c8194d9c813802940"
                    .into(),
                description: Some(
                    "Zipfile with FGDB for City of Richland Public Works \
                     Water Utilities."
                        .into()
                ),
                publisher: Some("City of Richland, Washington".into()),
                doi: None,
                license: None,
                updated: Some("2026-09-21".into()),
                size_bytes: Some(3_740_026),
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(hits[1].license, None, "\"none\" is Hub's empty license");
    }
}
