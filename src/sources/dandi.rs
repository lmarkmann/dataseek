//! The DANDI Archive (neurophysiology), searched through its REST API. The
//! most recent published version names the dandiset; drafts fill in.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.dandiarchive.org/api/dandisets/")
        .query("search", query)
        .query("page_size", limit.clamp(1, 100))
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
    let id = text(row, "/identifier")?;
    let name = first_text(
        row,
        &["/most_recent_published_version/name", "/draft_version/name"],
    )?;
    let mut dataset = Dataset::new(
        &name,
        &format!("https://dandiarchive.org/dandiset/{id}"),
    );
    dataset.size_bytes = number(row, "/most_recent_published_version/size")
        .or_else(|| number(row, "/draft_version/size"));
    dataset.updated = day(text(row, "/modified"));
    dataset.valid()
}
