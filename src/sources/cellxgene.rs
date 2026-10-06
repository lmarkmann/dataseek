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
                publisher: None,
                doi: Some("10.1016/j.xgen.2023.100298".into()),
                license: None,
                updated: Some("2026-06-11".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
