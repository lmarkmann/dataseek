//! NOAA's National Centers for Environmental Information, through its
//! dataset search service.

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
        .get("https://www.ncei.noaa.gov/access/services/search/v1/datasets")
        .query("text", query)
        .query("limit", limit)
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
    let id = text(row, "/id")?;
    let landing = text(row, "/doiLink")
        .or_else(|| text(row, "/links/other/0/url"))
        .unwrap_or_else(|| {
            format!("https://www.ncei.noaa.gov/access/search/dataset-search?text={id}")
        });
    let mut dataset = Dataset::new(&text(row, "/name")?, &landing)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/doiLink"));
    dataset.publisher = Some("NOAA NCEI".to_owned());
    dataset.updated = text(row, "/endDate");
    dataset.valid()
}
