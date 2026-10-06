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
    dataset.license = text(props, "/license").filter(|l| l != "custom");
    dataset.updated = number(props, "/modified").and_then(date_from_epoch);
    dataset.valid()
}
