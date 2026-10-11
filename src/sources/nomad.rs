//! NOMAD's published datasets (computational materials science), paged out
//! of the datasets endpoint and searched locally: the endpoint filters by
//! exact name (`dataset_name`) or name prefix (`prefix`), never by words.
//!
//! There are 2,121 datasets, so `page_size=1000` takes three requests that
//! follow `next_page_after_value`, and the loop gives up after 20. NOMAD says
//! many endpoints enforce a maximum page size; this one accepted 100,000. The
//! listing has no description, and `dataset_modified_time` equals
//! `dataset_create_time` wherever it is present, so the creation time is the
//! update date. Requests are limited per IP address, "as low as 30 requests
//! per second or 10 concurrent requests" (NOMAD documentation), and metadata
//! may be reused under CC0 (NOMAD terms of use). All as of October 2026.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..20 {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let (entries, next) = parse(&fixture::json("nomad.json"));
        assert_eq!(entries.len(), 4);
        assert_eq!(next.as_deref(), Some("999"));
        assert_eq!(
            entries[0],
            Dataset {
                title: "demo example data".into(),
                url: "https://nomad-lab.eu/prod/v1/gui/dataset/id/\
                      wWgAnNZNQxOHLf97H62dgw"
                    .into(),
                description: None,
                publisher: None,
                doi: Some("10.17172/nomad/2020.05.20-1".into()),
                license: None,
                updated: Some("2020-05-20".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_last_page_names_no_cursor() {
        let (entries, next) = parse(&fixture::json("nomad.last.json"));
        assert_eq!(next, None);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[1],
            Dataset {
                title: "Mercury intrusion porosimetry as a quantitative tool \
                        for the shape estimation of supraparticles generated \
                        via spray-drying"
                    .into(),
                url: "https://nomad-lab.eu/prod/v1/gui/dataset/id/\
                      EIkdAcj0R0yiggOQu5ALAA"
                    .into(),
                description: None,
                publisher: None,
                doi: Some("10.17172/nomad.pnez-s893".into()),
                license: None,
                updated: Some("2026-06-04".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
