//! World Bank indicators (about 29,500), downloaded in one request and
//! searched locally; the indicator API has no search parameter. Descriptions
//! are cut short to keep the cached catalog small.

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.worldbank.org/v2/indicator")
        .query("format", "json")
        .query("per_page", 40_000)
        .slow()
        .json()?;
    let rows = items(&body, "/1");
    if rows.is_empty() {
        return Err(SourceError::shape("no indicator list"));
    }
    Ok(rows
        .iter()
        .filter_map(|row| {
            let id = text(row, "/id")?;
            let teaser = text(row, "/sourceNote")
                .map(|note| note.chars().take(140).collect::<String>());
            let mut dataset = Dataset::new(
                &text(row, "/name")?,
                &format!("https://data.worldbank.org/indicator/{id}"),
            )
            .describe(teaser);
            dataset.publisher = text(row, "/source/value");
            dataset.valid()
        })
        .collect())
}
