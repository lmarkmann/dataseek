//! OmicsDI, the Omics Discovery Index across genomics, proteomics,
//! metabolomics and transcriptomics repositories.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://www.omicsdi.org/ws/dataset/search")
        .query("query", query)
        .query("size", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("datasets").is_none() {
        return Err(SourceError::shape("no datasets array"));
    }
    Ok(items(body, "/datasets")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let repository = text(row, "/source")?;
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://www.omicsdi.org/dataset/{repository}/{id}"),
    )
    .describe(text(row, "/description"));
    dataset.publisher = Some(repository.replace('_', " "));
    dataset.updated = day(text(row, "/publicationDate"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("omicsdi.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Low-temperature induced neuro-oxidative stress and \
                        metabolic reprogramming in the brain of juvenile \
                        fourfinger threadfin (Eleutheronema tetradactylum)"
                    .into(),
                url: "https://www.omicsdi.org/dataset/metabolights_dataset/\
                      MTBLS15103"
                    .into(),
                description: Some(
                    "This study investigated the effects of low-temperature \
                     stress on the brain metabolome of juvenile fourfinger \
                     threadfin (Eleutheronema tetradactylum). Using \
                     UHPLC-Q-TOF-MS-based untargeted metabolomics, we \
                     analyzed brain tissues from fish exposed to 28\u{b0}C \
                     (control) and 18\u{b0}C for 7 and 14 days."
                        .into()
                ),
                publisher: Some("metabolights dataset".into()),
                doi: None,
                license: None,
                updated: Some("2026-07-22".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
