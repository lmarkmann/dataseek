//! Mendeley Data, through the search endpoint its own site uses. The
//! documented API (OAuth) has no free-text search, only DOI and ISSN
//! filters, so an OAuth token would not improve what this adapter can find.

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
        .get("https://data.mendeley.com/api/research-data/search")
        .query("search", query)
        .query("size", limit)
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("records").is_none() {
        return Err(SourceError::shape("no records array"));
    }
    Ok(items(body, "/records").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset = Dataset::new(&text(row, "/title")?, &text(row, "/url")?)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/doi/0").or_else(|| text(row, "/doi")));
    dataset.publisher = text(row, "/source/name");
    dataset.updated = day(text(row, "/publication_date"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("mendeley.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Harmonized Multi-Source Dataset of Agricultural \
                        Commodity Prices, Meteorological Variations, and \
                        Macroeconomic Indicators for Mali"
                    .into(),
                url: "https://data.mendeley.com/datasets/ygfjw7ym7j".into(),
                description: Some(
                    "This dataset provides a comprehensive, multi-variable \
                     panel combining agricultural market commodity prices, \
                     meteorological factors, and macroeconomic indicators \
                     across major administrative regions and markets in \
                     Mali."
                        .into()
                ),
                publisher: Some("Mendeley Data".into()),
                doi: Some("10.17632/ygfjw7ym7j".into()),
                license: None,
                updated: Some("2026-10-06".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
