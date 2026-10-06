//! Kaggle dataset search through the official API. The user's own token is
//! sent when one is found (see `credentials.rs`); without it the endpoint
//! still answers its first page of 20. Results are never cached to disk,
//! because Kaggle's terms forbid storing a significant portion of content.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let mut call = ctx
        .http
        .get("https://www.kaggle.com/api/v1/datasets/list")
        .query("search", query)
        .query("page", 1);
    if let Some(secret) = ctx.creds.get(Key::Kaggle) {
        call = call.header("Authorization", secret.authorization());
    }
    let body = call.json()?;
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of datasets"))?;
    Ok(rows.iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let reference = text(row, "/ref")?;
    let url = text(row, "/url").unwrap_or_else(|| {
        format!("https://www.kaggle.com/datasets/{reference}")
    });
    let mut dataset = Dataset::new(&text(row, "/title")?, &url)
        .describe(text(row, "/subtitle"));
    dataset.publisher = text(row, "/ownerName");
    dataset.license = text(row, "/licenseName");
    dataset.updated = day(text(row, "/lastUpdated"));
    dataset.size_bytes = number(row, "/totalBytes");
    dataset.popularity = number(row, "/downloadCount");
    dataset.valid()
}
