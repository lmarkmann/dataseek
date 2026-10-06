//! CERN Open Data (Invenio), limited to records of type Dataset.

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
        .get("https://opendata.cern.ch/api/records/")
        .query("q", query)
        .query("type", "Dataset")
        .query("size", limit.clamp(1, 100))
        .json()?;
    if body.pointer("/hits/hits").is_none() {
        return Err(SourceError::shape("no hits.hits"));
    }
    Ok(items(&body, "/hits/hits")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(hit: &Value) -> Option<Dataset> {
    let meta = hit.get("metadata")?;
    let recid = text(meta, "/recid").or_else(|| text(hit, "/id"))?;
    let mut dataset = Dataset::new(
        &text(meta, "/title")?,
        &format!("https://opendata.cern.ch/record/{recid}"),
    )
    .describe(text(meta, "/abstract/description"))
    .doi_from(text(meta, "/doi"));
    dataset.publisher = text(meta, "/experiment/0")
        .or_else(|| text(meta, "/experiment"))
        .map(|e| format!("CERN {e}"));
    dataset.updated = text(meta, "/date_published");
    dataset.valid()
}
