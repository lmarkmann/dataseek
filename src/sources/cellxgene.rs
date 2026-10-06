//! CZ CELLxGENE Discover collections (single-cell atlases), listed through
//! the Curation API and searched locally.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.cellxgene.cziscience.com/curation/v1/collections")
        .slow()
        .json()?;
    parse(&body)
}

pub(super) fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of collections"))?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let mut dataset = Dataset::new(
                &text(row, "/name")?,
                &text(row, "/collection_url")?,
            )
            .describe(text(row, "/description"))
            .doi_from(text(row, "/doi"));
            dataset.updated = day(text(row, "/revised_at")
                .or_else(|| text(row, "/published_at")));
            dataset.valid()
        })
        .collect())
}
