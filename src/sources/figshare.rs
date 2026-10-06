//! Figshare's public article search, item type 3 (dataset). Covers
//! figshare.com and the institutional portals on the same platform. The
//! search hits carry no description; the landing page has it.

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .post("https://api.figshare.com/v2/articles/search")
        .json_body(json!({
            "search_for": query,
            "item_type": 3,
            "page_size": limit.clamp(1, 100),
        }))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of articles"))?;
    Ok(rows.iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset =
        Dataset::new(&text(row, "/title")?, &text(row, "/url_public_html")?)
            .doi_from(text(row, "/doi"));
    dataset.updated = day(text(row, "/published_date"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("figshare.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Data Set for 2023_67023_39501".into(),
                url: "https://figshare.com/articles/dataset/\
                      Data_Set_for_2023_67023_39501/34099329"
                    .into(),
                description: None,
                publisher: None,
                doi: Some("10.6084/m9.figshare.34099329.v1".into()),
                license: None,
                updated: Some("2026-10-06".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[1].title,
            "Data and code for Ecological zoning adds conditional \
             information on productivity in persistent forest-dominated \
             locations in China"
        );
    }
}
