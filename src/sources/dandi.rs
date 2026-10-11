//! The DANDI Archive (neurophysiology), searched through its REST API. The
//! most recent published version names the dandiset; drafts fill in.
//!
//! The listing has no relevance order: `search` keeps the dandisets whose
//! version metadata contains every word, ignoring case, and the default order
//! is oldest first. Starred dandisets come first instead, the one quality
//! signal the listing carries. `page_size` goes up to 1000, so a page never
//! needs paging. `empty=false` drops dandisets without files, which are
//! drafts that report a size of 0 (DANDI, October 2026).
//!
//! A word with a colon is read as a `key:value` filter, and an unknown key or
//! an unbalanced quote answers 400, so quote marks are dropped and a word with
//! a colon is sent quoted (DANDI, October 2026).
//!
//! The listing carries no description or license: each takes one more request
//! per version, which is why they stay empty. A published version has the DOI
//! `10.48324/dandi.<id>/<version>`, the prefix being the one `/api/info/`
//! reports (DANDI, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, number, text};

const DOI_PREFIX: &str = "10.48324";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.dandiarchive.org/api/dandisets/")
        .query("search", plain_words(query))
        .query("ordering", "-stars")
        .query("empty", false)
        .query("page_size", limit.clamp(1, 1000))
        .json()?;
    parse(&body, limit)
}

fn plain_words(query: &str) -> String {
    query
        .replace('"', " ")
        .split_whitespace()
        .map(|word| {
            if word.contains(':') {
                format!("\"{word}\"")
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    )
    .doi_from(
        text(row, "/most_recent_published_version/version")
            .map(|version| format!("{DOI_PREFIX}/dandi.{id}/{version}")),
    );
    dataset.popularity = number(row, "/star_count");
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
                title: "Human brain cell census for BA 44/45".into(),
                url: "https://dandiarchive.org/dandiset/000026".into(),
                description: None,
                publisher: None,
                doi: None,
                license: None,
                updated: Some("2026-03-25".into()),
                size_bytes: Some(38_464_536_222_290),
                popularity: Some(6),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_published_version_carries_its_doi() {
        let hits = parse(&fixture::json("dandi.json"), 10).unwrap();
        assert_eq!(
            hits[1].doi.as_deref(),
            Some("10.48324/dandi.000623/0.240227.2023")
        );
        assert_eq!(hits[1].popularity, Some(4));
    }

    #[test]
    fn a_query_cannot_reach_the_archives_filter_syntax() {
        for (query, sent) in [
            ("hippocampus place cells", "hippocampus place cells"),
            ("species:mouse", "\"species:mouse\""),
            (
                "brain https://doi.org/10.48324/dandi.000004",
                "brain \"https://doi.org/10.48324/dandi.000004\"",
            ),
            ("sea \"ice", "sea ice"),
            ("\"sea ice\"  cores", "sea ice cores"),
        ] {
            assert_eq!(plain_words(query), sent, "{query}");
        }
    }
}
