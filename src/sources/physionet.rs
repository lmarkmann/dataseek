//! PhysioNet's published projects (latest versions only), from the JSON list
//! its site exposes, searched locally. Many need credentialed access to
//! download; the listing itself is public.
//!
//! The list is `/api/v1/projects/published/`; the singular
//! `/api/v1/project/published/` this adapter used answers the same bytes but
//! PhysioNet's code calls it a legacy synonym (physionet-build,
//! `export/urls.py`, October 2026). One response holds every published
//! version, 736 projects of which 546 are the latest, unpaginated and about
//! 1.3 MB; `page` and `limit` are ignored and the server takes about 30
//! seconds to build it (October 2026). Its code throttles anonymous clients
//! to 20 requests an hour per address (`export/views.py`), which one catalog
//! download a week stays far under.
//!
//! Projects come in four resource types. Databases and challenges are
//! datasets; software and models are not, and are dropped (17 of the 144
//! latest projects matching "ecg" are software, October 2026). The list has
//! no modification date, so `updated` is the publish date of the version
//! listed. A project with a version DOI and a concept DOI carries the
//! concept DOI as an alias so the other sources' copies merge.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, first_text, number, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://physionet.org/api/v1/projects/published/")
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
        .filter(|row| {
            !matches!(
                text(row, "/resource_type").as_deref(),
                Some("Software" | "Model")
            )
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
            dataset.aliases = text(row, "/core_doi")
                .as_deref()
                .and_then(doi)
                .filter(|concept| dataset.doi.as_ref() != Some(concept))
                .into_iter()
                .collect();
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
        assert_eq!(entries.len(), 4);
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
        assert_eq!(
            entries[3],
            Dataset {
                title: "VOICED Database".into(),
                url: "https://physionet.org/content/voiced/1.0.0/".into(),
                description: Some(
                    "This database includes 208 voice samples, from 150 \
                     pathological, and 58 healthy voices."
                        .into()
                ),
                publisher: None,
                doi: Some("10.13026/c25q2n".into()),
                license: Some(
                    "Open Data Commons Attribution License v1.0".into()
                ),
                updated: Some("2018-06-07".into()),
                size_bytes: Some(115_343_616),
                popularity: None,
                aliases: vec!["10.13026/twfd-kb89".into()],
            }
        );
    }

    #[test]
    fn older_versions_software_and_models_are_left_out() {
        let entries = parse(&fixture::json("physionet.json")).unwrap();
        let urls: Vec<&str> = entries.iter().map(|d| d.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://physionet.org/content/slpdb/1.0.0/",
                "https://physionet.org/content/stdb/1.0.0/",
                "https://physionet.org/content/cdb/1.0.0/",
                "https://physionet.org/content/voiced/1.0.0/",
            ]
        );
    }
}
