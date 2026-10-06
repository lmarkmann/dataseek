//! NOMAD's published datasets (computational materials science), paged out
//! of the datasets endpoint and searched locally: the endpoint filters by
//! exact name only.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..20 {
        let mut call = ctx
            .http
            .get("https://nomad-lab.eu/prod/v1/api/v1/datasets/")
            .query("page_size", 1000)
            .slow();
        if let Some(cursor) = &after {
            call = call.query("page_after_value", cursor);
        }
        let body = call.json()?;
        let (page, next) = parse(&body);
        entries.extend(page);
        match next {
            Some(next) if after.as_ref() != Some(&next) => after = Some(next),
            _ => break,
        }
    }
    Ok(entries)
}

/// One page of datasets and the cursor of the next page, if any.
pub(super) fn parse(body: &Value) -> (Vec<Dataset>, Option<String>) {
    let entries = items(body, "/data")
        .iter()
        .filter_map(|row| {
            let id = text(row, "/dataset_id")?;
            let mut dataset = Dataset::new(
                &text(row, "/dataset_name")?,
                &format!("https://nomad-lab.eu/prod/v1/gui/dataset/id/{id}"),
            )
            .doi_from(text(row, "/doi"));
            dataset.updated = day(text(row, "/dataset_create_time"));
            dataset.valid()
        })
        .collect();
    (entries, text(body, "/pagination/next_page_after_value"))
}
