//! GBIF's dataset registry search: occurrence, checklist and sampling-event
//! datasets from biodiversity publishers worldwide.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.gbif.org/v1/dataset/search")
        .query("q", query)
        .query("limit", limit.clamp(1, 1000))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(body, "/results").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let key = text(row, "/key")?;
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://www.gbif.org/dataset/{key}"),
    )
    .describe(text(row, "/description"))
    .doi_from(text(row, "/doi"));
    dataset.publisher = text(row, "/publishingOrganizationTitle");
    dataset.license = text(row, "/license");
    dataset.valid()
}
