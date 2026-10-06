//! Mendeley Data, through the search endpoint its own site uses. The
//! documented API (OAuth) has no free-text search, only DOI and ISSN
//! filters, so an OAuth token would not improve what this adapter can find.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://data.mendeley.com/api/research-data/search")
        .query("search", query)
        .query("size", limit)
        .json()?;
    if body.get("records").is_none() {
        return Err(SourceError::shape("no records array"));
    }
    Ok(items(&body, "/records")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset = Dataset::new(&text(row, "/title")?, &text(row, "/url")?)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/doi/0").or_else(|| text(row, "/doi")));
    dataset.publisher = text(row, "/source/name");
    dataset.updated = day(text(row, "/publication_date"));
    dataset.valid()
}
