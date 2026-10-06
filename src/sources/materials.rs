//! Materials Project contributed datasets (MPContribs projects), listed
//! keyless and searched locally. The core Materials Project API is a
//! per-material database rather than a dataset catalog, so it is not used.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://contribs-api.materialsproject.org/projects/")
        .query("_fields", "name,title,description")
        .query("_limit", 500)
        .slow()
        .json()?;
    Ok(parse(&body))
}

pub(super) fn parse(body: &Value) -> Vec<Dataset> {
    items(body, "/data")
        .iter()
        .filter_map(|row| {
            let name = text(row, "/name")?;
            let title = text(row, "/title").unwrap_or_else(|| name.clone());
            Dataset::new(
                &title,
                &format!(
                    "https://contribs.materialsproject.org/projects/{name}"
                ),
            )
            .describe(text(row, "/description"))
            .valid()
        })
        .collect()
}
