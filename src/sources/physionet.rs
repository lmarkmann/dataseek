//! PhysioNet's published projects (latest versions only), from the JSON list
//! its site exposes, searched locally. Many need credentialed access to
//! download; the listing itself is public.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, number, text};

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
            dataset.license = text(row, "/license/name");
            dataset.size_bytes = number(row, "/main_storage_size");
            dataset.updated = day(text(row, "/publish_date"));
            dataset.valid()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let entries = parse(&fixture::json("physionet.json")).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            Dataset {
                title: "MIT-BIH Polysomnographic Database".into(),
                url: "https://physionet.org/content/slpdb/1.0.0/".into(),
                description: Some(
                    "The MIT-BIH Polysomnographic Database is a collection of \
                     recordings of multiple physiologic signals during sleep."
                        .into()
                ),
                publisher: None,
                doi: Some("10.13026/c23k5s".into()),
                license: Some(
                    "Open Data Commons Attribution License v1.0".into()
                ),
                updated: Some("1999-08-03".into()),
                size_bytes: Some(663_056_564),
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
