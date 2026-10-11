//! The Dataverse Search API, identical on every installation; a registry row
//! per installation picks the base URL.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{base}/api/search"))
        .query("q", query)
        .query("type", "dataset")
        .query("per_page", limit.clamp(1, 1000))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/data/items").is_none() {
        return Err(SourceError::shape("no data.items"));
    }
    Ok(items(body, "/data/items")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset = Dataset::new(&text(row, "/name")?, &text(row, "/url")?)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/global_id"));
    dataset.publisher = text(row, "/publisher");
    dataset.updated = day(first_text(row, &["/updatedAt", "/published_at"]));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("dataverse.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Climatic characterization of the sites in Argentina \
                        (C\u{f3}rdoba y Santiago del Estero)"
                    .into(),
                url: "https://doi.org/10.7910/DVN/DUWBBU".into(),
                description: Some(
                    "Dateset contains past and future exploration on climate \
                     in Chancan\u{ed}, C\u{f3}rdoba and Copo, Santiago del \
                     Estero, Argentina. The described variables were \
                     proposed by the climatologist, biologist and agronomic \
                     engineers and up -dated with the information obtained \
                     after the stakeholders workshop."
                        .into()
                ),
                publisher: Some("Climate variability in the Americas".into()),
                doi: Some("10.7910/dvn/duwbbu".into()),
                license: None,
                updated: Some("2025-06-16".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
