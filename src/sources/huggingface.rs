//! Hugging Face Hub dataset search. Sorted by downloads: the Hub has no
//! relevance ranking, and the most used match is the useful default.
//!
//! The Hub's `search` matches a substring of the repository id, so
//! "climate temperature" finds almost nothing. A multi-word query therefore
//! asks for the longest word, takes the 100 most downloaded matches, and
//! keeps those whose id, description or tags contain every word.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let words: Vec<String> =
        query.split_whitespace().map(str::to_lowercase).collect();
    let anchor =
        words.iter().max_by_key(|w| w.len()).cloned().unwrap_or_default();
    let wide = words.len() > 1;
    let mut call = ctx
        .http
        .get("https://huggingface.co/api/datasets")
        .query("search", anchor)
        .query("sort", "downloads")
        .query("limit", if wide { 100 } else { limit });
    if let Some(secret) = ctx.creds.get(Key::HuggingFace) {
        call = call.header("Authorization", secret.authorization());
    }
    let body = call.json()?;
    parse(&body, &words, limit)
}

pub(super) fn parse(
    body: &Value,
    words: &[String],
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let wide = words.len() > 1;
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of datasets"))?;
    Ok(rows
        .iter()
        .filter(|row| !wide || mentions_all(row, words))
        .filter_map(record)
        .take(limit)
        .collect())
}

fn mentions_all(row: &Value, words: &[String]) -> bool {
    let mut haystack = text(row, "/id").unwrap_or_default();
    haystack.push(' ');
    haystack.push_str(&text(row, "/description").unwrap_or_default());
    for tag in items(row, "/tags").iter().filter_map(Value::as_str) {
        haystack.push(' ');
        haystack.push_str(tag);
    }
    let haystack = haystack.to_lowercase();
    words.iter().all(|w| haystack.contains(w.as_str()))
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let mut dataset =
        Dataset::new(&id, &format!("https://huggingface.co/datasets/{id}"))
            .describe(text(row, "/description"));
    dataset.publisher = text(row, "/author");
    dataset.updated = day(text(row, "/lastModified"));
    dataset.popularity = number(row, "/downloads");
    dataset.license = items(row, "/tags")
        .iter()
        .filter_map(Value::as_str)
        .find_map(|tag| tag.strip_prefix("license:"))
        .map(str::to_owned);
    dataset.valid()
}
