//! Mendeley Data, through the search endpoint its own site uses. The same
//! search is documented as `GET /search` on api.data.mendeley.com, but that
//! host wants an OAuth token for a client registered by e-mail (Digital
//! Commons Data API, October 2026). The site endpoint answers anonymously;
//! its robots.txt disallows `/api/` and its terms ask for written permission
//! before automated access (data.mendeley.com, October 2026).
//!
//! `query` is at most 255 characters, and one more makes the server answer
//! HTTP 500, which would park the source as down. `page_size` is at most 500
//! and `page` times `page_size` at most 10,000 (Digital Commons Data API and
//! a live probe, October 2026), so one page covers any `limit`.
//! `publication_date` is the newest version's, which makes it the date of the
//! last change (OAI-PMH datestamps of versions, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

const QUERY_CHARS: usize = 255;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://data.mendeley.com/api/research-data/search")
        .query("query", clipped(query))
        .query("page_size", limit)
        .json()?;
    parse(&body, limit)
}

/// The query cut to the longest one the endpoint takes, at a word.
fn clipped(query: &str) -> &str {
    let Some((end, next)) = query.char_indices().nth(QUERY_CHARS) else {
        return query;
    };
    let head = query.get(..end).unwrap_or(query);
    if next.is_whitespace() {
        return head;
    }
    head.rsplit_once(char::is_whitespace).map_or(head, |(words, _)| words)
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
        .doi_from(text(row, "/doi/0"));
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

    #[test]
    fn a_query_the_endpoint_accepts_is_sent_as_typed() {
        assert_eq!(
            clipped("sea surface temperature"),
            "sea surface temperature"
        );
        let longest = "é".repeat(QUERY_CHARS);
        assert_eq!(clipped(&longest), longest);
    }

    #[test]
    fn a_longer_query_is_cut_at_a_word_within_255_characters() {
        let long = "water ".repeat(60);
        assert_eq!(clipped(&long), ["water"; 42].join(" "));

        let ends_on_a_word = format!("{} tail", "a".repeat(QUERY_CHARS));
        assert_eq!(clipped(&ends_on_a_word), "a".repeat(QUERY_CHARS));

        let one_word = "é".repeat(300);
        assert_eq!(clipped(&one_word).chars().count(), QUERY_CHARS);
    }
}
