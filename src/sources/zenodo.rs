//! Zenodo's records search (InvenioRDM), limited to resource type dataset.
//! Anonymous pages are capped at 25 and every client at 30 searches a minute
//! (Zenodo, November 2025). The concept DOI travels as an alias so versions
//! of one record merge.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://zenodo.org/api/records")
        .query("q", query)
        .query("type", "dataset")
        .query("size", limit.clamp(1, 25))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/hits/hits").is_none() {
        return Err(SourceError::shape("no hits.hits"));
    }
    Ok(items(body, "/hits/hits")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let meta = row.get("metadata")?;
    let url = text(row, "/links/self_html").or_else(|| {
        text(row, "/id").map(|id| format!("https://zenodo.org/records/{id}"))
    })?;
    let mut dataset = Dataset::new(&text(meta, "/title")?, &url)
        .describe(text(meta, "/description"))
        .doi_from(text(row, "/doi"));
    dataset.publisher = text(meta, "/creators/0/name");
    dataset.license = text(meta, "/license/id");
    dataset.updated = day(text(row, "/updated"));
    let sizes: Vec<u64> = items(row, "/files")
        .iter()
        .filter_map(|f| number(f, "/size"))
        .collect();
    if !sizes.is_empty() {
        dataset.size_bytes =
            Some(sizes.iter().fold(0, |a, b| a.saturating_add(*b)));
    }
    dataset.popularity = number(row, "/stats/unique_downloads");
    if let Some(concept) = text(row, "/conceptdoi") {
        dataset.aliases.push(concept);
    }
    dataset.valid()
}
