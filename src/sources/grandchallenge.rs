//! Grand Challenge's medical-imaging challenges, paged out of its REST API
//! and searched locally. Each challenge has its own subdomain, a short
//! description and the date it was last modified.
//!
//! Pages of 100 follow `next` (264 challenges in three pages, November
//! 2026), and the loop gives up after 20. The oldest challenges have an
//! empty `title`, so their slug ("VESSEL12") is the title. `publications`
//! holds the DOIs of the papers about a challenge, never one of the data, so
//! it is not read as the record's DOI. robots.txt disallows `/api/` for
//! crawlers, against the documented API that the `gcapi` client calls; one
//! catalog download a week is the whole load.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, text};

const API: &str = "https://grand-challenge.org/api/v1/challenges/";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    let mut page = format!("{API}?limit=100");
    for _ in 0..20 {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        let body = ctx.http.get(&page).slow().json()?;
        let (challenges, next) = parse(&body)?;
        entries.extend(challenges);
        match next {
            Some(next) if next.starts_with(API) => page = next,
            _ => break,
        }
    }
    Ok(entries)
}

/// One page of challenges and the link to the next page, if any.
fn parse(body: &Value) -> Result<(Vec<Dataset>, Option<String>), SourceError> {
    if body.pointer("/results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    let entries = items(body, "/results")
        .iter()
        .filter(|row| {
            row.get("public").and_then(Value::as_bool) != Some(false)
        })
        .filter_map(|row| {
            let mut dataset = Dataset::new(
                &first_text(row, &["/title", "/slug"])?,
                &text(row, "/url")?,
            )
            .describe(text(row, "/description"));
            dataset.updated = day(text(row, "/modified"));
            dataset.valid()
        })
        .collect();
    Ok((entries, text(body, "/next")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let (hits, next) =
            parse(&fixture::json("grandchallenge.json")).unwrap();
        assert_eq!(
            next.as_deref(),
            Some(
                "https://grand-challenge.org/api/v1/challenges/?limit=100&offset=100"
            )
        );
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[1].title, "Subsolid Nodules");
        assert_eq!(
            hits[0],
            Dataset {
                title: "VESSEL12".into(),
                url: "https://vessel12.grand-challenge.org/".into(),
                description: Some(
                    "The VESSEL12 challenge compares methods for automatic \
                     (and semi-automatic) segmentation of blood vessels in \
                     the lungs from CT images."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: Some("2025-07-16".into()),
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_body_without_results_is_a_changed_shape() {
        let changed = serde_json::json!({"error": "unexpected"});
        assert!(matches!(parse(&changed), Err(SourceError::Shape(_))));
    }
}
