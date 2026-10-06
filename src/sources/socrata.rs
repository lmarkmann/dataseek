//! Socrata's Discovery API: one endpoint per region that searches every
//! public Socrata domain (US cities and states, data.nasa.gov,
//! healthdata.gov, datos.gov.co, ...), including column names.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{base}/api/catalog/v1"))
        .query("q", query)
        .query("only", "dataset")
        .query("limit", limit)
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
    let url = text(row, "/link").or_else(|| text(row, "/permalink"))?;
    let mut dataset = Dataset::new(&text(row, "/resource/name")?, &url)
        .describe(text(row, "/resource/description"));
    dataset.publisher = text(row, "/resource/attribution")
        .or_else(|| text(row, "/metadata/domain"));
    dataset.license = text(row, "/metadata/license");
    dataset.updated = day(text(row, "/resource/updatedAt"));
    dataset.popularity = number(row, "/resource/download_count");
    if let Some(permalink) = text(row, "/permalink") {
        dataset.aliases.push(permalink);
    }
    dataset.valid()
}
