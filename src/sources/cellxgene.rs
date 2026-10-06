//! CZ CELLxGENE Discover collections (single-cell atlases), listed through
//! the Curation API and searched locally. The list takes only `visibility`
//! and `curator`, has no search or paging parameters, and answered all 397
//! public collections in one 3 MB body (CELLxGENE Curation API, October
//! 2026). A collection's `doi` is its paper's, shared by up to three
//! collections, so it is left out: dedup would fold those into one hit. The
//! consortia are contributing research programmes, not publishers; the
//! platform is the publisher, and its data submission policy puts every
//! published dataset under CC BY 4.0 (CELLxGENE, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, text};

const PUBLISHER: &str = "CZ CELLxGENE Discover";
const LICENSE: &str = "CC-BY-4.0";

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
            .describe(text(row, "/description"));
            dataset.publisher = Some(PUBLISHER.to_owned());
            dataset.license = Some(LICENSE.to_owned());
            dataset.updated = day(text(row, "/revised_at")
                .or_else(|| text(row, "/published_at")));
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
        let entries = parse(&fixture::json("cellxgene.json")).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries[0],
            Dataset {
                title:
                    "Single-cell transcriptomic atlas for adult human retina"
                        .into(),
                url: "https://cellxgene.cziscience.com/collections/\
                      af893e86-8e9f-41f1-a474-ef05359b1fb7"
                    .into(),
                description: Some(
                    "The retina is the innermost tissue of the eyes of human \
                     and most other vertebrates. It receives the information \
                     of the visual images like the film of a camera and then \
                     translates the images into neural signals."
                        .into()
                ),
                publisher: Some("CZ CELLxGENE Discover".into()),
                doi: None,
                license: Some("CC-BY-4.0".into()),
                updated: Some("2026-06-11".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn collections_of_one_paper_stay_separate_hits() {
        let entries = parse(&fixture::json("cellxgene.json")).unwrap();
        assert_eq!(entries[1].title, "Human Immune Health Atlas");
        assert_eq!(
            entries[2].title,
            "Multi-omic profiling reveals age-related immune dynamics in \
             healthy adults"
        );
        let hits = crate::dedup::merge(&[("cellxgene", entries)]);
        assert_eq!(hits.len(), 4);
    }

    #[test]
    fn a_collection_never_revised_dates_from_its_publication() {
        let entries = parse(&fixture::json("cellxgene.json")).unwrap();
        assert_eq!(entries[3].updated.as_deref(), Some("2026-09-08"));
    }
}
