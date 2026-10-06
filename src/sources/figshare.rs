//! Figshare's public article search, item type 3 (dataset). Covers
//! figshare.com and the institutional portals on the same platform. The
//! search hits carry no description; the landing page has it.

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .post("https://api.figshare.com/v2/articles/search")
        .json_body(json!({
            "search_for": query,
            "item_type": 3,
            "page_size": limit.clamp(1, 100),
        }))
        .json()?;
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of articles"))?;
    Ok(rows.iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset =
        Dataset::new(&text(row, "/title")?, &text(row, "/url_public_html")?)
            .doi_from(text(row, "/doi"));
    dataset.updated = day(text(row, "/published_date"));
    dataset.valid()
}
