//! OmicsDI, the Omics Discovery Index across genomics, proteomics,
//! metabolomics and transcriptomics repositories.

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
        .get("https://www.omicsdi.org/ws/dataset/search")
        .query("query", query)
        .query("size", limit.clamp(1, 100))
        .json()?;
    if body.get("datasets").is_none() {
        return Err(SourceError::shape("no datasets array"));
    }
    Ok(items(&body, "/datasets")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let repository = text(row, "/source")?;
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://www.omicsdi.org/dataset/{repository}/{id}"),
    )
    .describe(text(row, "/description"));
    dataset.publisher = Some(repository.replace('_', " "));
    dataset.updated = text(row, "/publicationDate").map(|d| compact_date(&d));
    dataset.valid()
}

/// `"20220124"` as `"2022-01-24"`; anything else unchanged.
fn compact_date(raw: &str) -> String {
    match (raw.get(0..4), raw.get(4..6), raw.get(6..8)) {
        (Some(y), Some(m), Some(d))
            if raw.len() == 8 && raw.chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("{y}-{m}-{d}")
        }
        _ => raw.to_owned(),
    }
}
