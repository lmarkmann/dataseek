//! PhysioNet's published projects (latest versions only), from the JSON list
//! its site exposes, searched locally. Many need credentialed access to
//! download; the listing itself is public.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://physionet.org/api/v1/project/published/")
        .slow()
        .json()?;
    parse(&body)
}

pub(super) fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of projects"))?;
    Ok(rows
        .iter()
        .filter(|row| {
            row.get("is_latest_version").and_then(Value::as_bool)
                != Some(false)
        })
        .filter_map(|row| {
            let slug = text(row, "/slug")?;
            let version = text(row, "/version")?;
            let mut dataset = Dataset::new(
                &text(row, "/title")?,
                &format!("https://physionet.org/content/{slug}/{version}/"),
            )
            .describe(text(row, "/abstract"))
            .doi_from(first_text(row, &["/version_doi", "/core_doi"]));
            dataset.updated = day(text(row, "/publish_datetime"));
            dataset.valid()
        })
        .collect())
}
