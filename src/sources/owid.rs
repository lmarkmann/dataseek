//! Our World in Data, through the search endpoint its site uses. Charts and
//! explorers only; articles are not datasets. The endpoint is not a
//! documented public API, so a shape change is expected one day and is
//! reported as such.

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
        .get("https://ourworldindata.org/api/search")
        .query("q", query)
        .json()?;
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(&body, "/results")
        .iter()
        .filter(|r| {
            matches!(
                r.get("type").and_then(Value::as_str),
                Some("chart" | "explorerView" | "multiDimView")
            )
        })
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let url = text(row, "/url").or_else(|| {
        text(row, "/slug")
            .map(|s| format!("https://ourworldindata.org/grapher/{s}"))
    })?;
    let mut dataset = Dataset::new(&text(row, "/title")?, &url)
        .describe(text(row, "/subtitle"));
    dataset.publisher = Some("Our World in Data".to_owned());
    dataset.license = Some("CC-BY-4.0".to_owned());
    dataset.updated = day(text(row, "/updatedAt"));
    dataset.valid()
}
