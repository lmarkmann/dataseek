//! The DANDI Archive (neurophysiology), searched through its REST API. The
//! most recent published version names the dandiset; drafts fill in.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.dandiarchive.org/api/dandisets/")
        .query("search", query)
        .query("page_size", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(body, "/results").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/identifier")?;
    let name = first_text(
        row,
        &["/most_recent_published_version/name", "/draft_version/name"],
    )?;
    let mut dataset = Dataset::new(
        &name,
        &format!("https://dandiarchive.org/dandiset/{id}"),
    );
    dataset.size_bytes = number(row, "/most_recent_published_version/size")
        .or_else(|| number(row, "/draft_version/size"));
    dataset.updated = day(first_text(
        row,
        &[
            "/most_recent_published_version/modified",
            "/draft_version/modified",
            "/modified",
        ],
    ));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("dandi.json"), 10).unwrap();
        assert_eq!(hits.len(), 5);
        assert_eq!(
            hits[0],
            Dataset {
                title: "A NWB-based dataset and processing pipeline of human \
                        single-neuron activity during a declarative memory task"
                    .into(),
                url: "https://dandiarchive.org/dandiset/000004".into(),
                description: None,
                publisher: None,
                doi: None,
                license: None,
                updated: Some("2022-01-26".into()),
                size_bytes: Some(6_197_474_020),
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
