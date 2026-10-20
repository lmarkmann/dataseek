//! The Dataverse Search API, identical on every installation; a registry row
//! per installation picks the base URL. Results come in relevance order
//! (`sort=score` is the default) and a page holds at most 1000 (a larger
//! `per_page` answers HTTP 400). The query matches any of its words in any
//! indexed field, the parent collection's name included, and tolerates
//! unbalanced quotes, brackets and operators. A dataset item carries no
//! license, size or download count, only `fileCount`; those live in the
//! per-dataset native API, one request each. The API states no rate limit
//! (Dataverse guides 6.12.1 and Harvard Dataverse 6.10.1, October 2026).
//! Harvard Dataverse includes harvested records from other archives (14348
//! of 18270 hits for "air quality"), whose link is their own DOI or handle
//! (Harvard Dataverse, October 2026).

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
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_query_matching_nothing_is_an_empty_answer() {
        let body = serde_json::json!({
            "status": "OK",
            "data": {
                "q": "zzqxjvkw qqxzjv",
                "total_count": 0,
                "start": 0,
                "spelling_alternatives": {},
                "items": [],
                "count_in_response": 0
            }
        });
        assert_eq!(parse(&body, 10).unwrap(), vec![]);
    }

    #[test]
    fn an_error_body_is_a_changed_shape() {
        let body = serde_json::json!({
            "status": "ERROR",
            "message": "1001 results per page requested but max limit is 1000."
        });
        assert!(matches!(parse(&body, 10), Err(SourceError::Shape(_))));
    }
}
