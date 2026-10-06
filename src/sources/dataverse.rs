//! The Dataverse Search API, identical on every installation; a registry row
//! per installation picks the base URL.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{base}/api/search"))
        .query("q", query)
        .query("type", "dataset")
        .query("per_page", limit.clamp(1, 1000))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/data/items").is_none() {
        return Err(SourceError::shape("no data.items"));
    }
    Ok(items(body, "/data/items")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset = Dataset::new(&text(row, "/name")?, &text(row, "/url")?)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/global_id"));
    dataset.publisher = text(row, "/publisher");
    dataset.updated = day(first_text(row, &["/updatedAt", "/published_at"]));
    dataset.valid()
}
