//! NASA's Common Metadata Repository: earth-science collections from NASA's
//! data centers and partner agencies. Search is keyless; downloads need an
//! Earthdata Login, which dataseek never touches.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://cmr.earthdata.nasa.gov/search/collections.json")
        .query("keyword", query)
        .query("page_size", limit.clamp(1, 2000))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/feed/entry").is_none() {
        return Err(SourceError::shape("no feed.entry"));
    }
    Ok(items(body, "/feed/entry")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(entry: &Value) -> Option<Dataset> {
    let id = text(entry, "/id")?;
    let mut dataset = Dataset::new(
        &text(entry, "/title")?,
        &format!("https://cmr.earthdata.nasa.gov/search/concepts/{id}.html"),
    )
    .describe(text(entry, "/summary"))
    .doi_from(text(entry, "/doi"));
    dataset.publisher = text(entry, "/data_center");
    dataset.updated = day(first_text(entry, &["/updated", "/time_start"]));
    dataset.valid()
}
