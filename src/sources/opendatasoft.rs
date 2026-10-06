//! The OpenDataSoft federated hub (data.opendatasoft.com), which indexes the
//! public datasets of OpenDataSoft portals: French cities and regions,
//! utilities, Swiss and UK councils. Explore API v2.1, ODSQL `search()`.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let literal: String = query.chars().filter(|c| *c != '"').collect();
    let body = ctx
        .http
        .get("https://data.opendatasoft.com/api/explore/v2.1/catalog/datasets")
        .query("where", format!("search(\"{literal}\")"))
        .query("limit", limit.clamp(1, 100))
        .json()?;
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(&body, "/results")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/dataset_id")?;
    let meta = row.pointer("/metas/default")?;
    let mut dataset = Dataset::new(
        &text(meta, "/title")?,
        &format!("https://data.opendatasoft.com/explore/dataset/{id}/"),
    )
    .describe(text(meta, "/description"));
    dataset.publisher = text(meta, "/publisher");
    dataset.license = text(meta, "/license");
    dataset.updated = day(text(meta, "/modified"));
    dataset.valid()
}
