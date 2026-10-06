//! The CESSDA Data Catalogue: European social science archives (UKDS, GESIS,
//! FSD, SND, ...) harvested into one search. The API refuses requests without
//! a metadata language; English is asked for.

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
        .get("https://datacatalogue.cessda.eu/api/DataSets/v2/search")
        .query("q", query)
        .query("limit", limit.clamp(1, 200))
        .query("metadataLanguage", "en")
        .json()?;
    if body.get("Results").is_none() {
        return Err(SourceError::shape("no Results array"));
    }
    Ok(items(&body, "/Results")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let mut dataset = Dataset::new(
        &text(row, "/titleStudy")?,
        &format!("https://datacatalogue.cessda.eu/detail/{id}?lang=en"),
    )
    .describe(text(row, "/abstract"));
    dataset.publisher = text(row, "/publisher/publisher");
    dataset.updated = day(text(row, "/lastModified"));
    if let Some(study) = text(row, "/studyUrl") {
        dataset.aliases.push(study);
    }
    dataset.valid()
}
