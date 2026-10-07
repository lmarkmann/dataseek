//! Our World in Data, through the Search API its own search box calls. Charts
//! and explorers only; articles are not datasets. The API is documented but
//! marked as under active development, so a shape change is expected one day
//! and is reported as such.
//!
//! `hitsPerPage` takes 1 to 100 against a default of 20, so the page is sized
//! to the count asked for and one request serves the largest per-source count
//! (OWID Search API docs, probes, October 2026). Results come in relevance
//! order. Subtitles are plain text: none of 1,000 results across ten queries
//! held Markdown or HTML (probes, October 2026). A chart's license is not in
//! the response, and the FAQ says most of the data behind OWID's charts
//! belongs to third parties under their own terms, so none is claimed (OWID
//! FAQs, October 2026). Charts that differ only by their data source share a
//! title, six of them "Life expectancy", and tell apart by `variantName`
//! (probes, October 2026). No rate limit is stated.

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
        .get("https://ourworldindata.org/api/search")
        .query("q", query)
        .query("hitsPerPage", limit.clamp(1, 100))
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
    Ok(items(body, "/results")
        .iter()
        .filter(|r| {
            matches!(
                r.get("type").and_then(Value::as_str),
                Some("chart" | "explorerView" | "multiDimView")
            )
        })
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let url = text(row, "/url").or_else(|| {
        text(row, "/slug")
            .map(|s| format!("https://ourworldindata.org/grapher/{s}"))
    })?;
    let title = text(row, "/title")?;
    let title = match text(row, "/variantName") {
        Some(variant) if variant != title => format!("{title} ({variant})"),
        _ => title,
    };
    let mut dataset =
        Dataset::new(&title, &url).describe(text(row, "/subtitle"));
    dataset.publisher = Some("Our World in Data".to_owned());
    dataset.updated = day(text(row, "/updatedAt"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("owid.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Sex gap in life expectancy".into(),
                url: "https://ourworldindata.org/grapher/\
                      difference-in-female-and-male-life-expectancy-at-birth"
                    .into(),
                description: Some(
                    "Difference between female and male life expectancy at \
                     birth. Positive values indicate higher female life \
                     expectancy; negative values indicate higher male life \
                     expectancy."
                        .into()
                ),
                publisher: Some("Our World in Data".into()),
                doi: None,
                license: None,
                updated: Some("2025-10-22".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_variant_name_tells_charts_with_one_title_apart() {
        let hits = parse(&fixture::json("owid.json"), 10).unwrap();
        let titles: Vec<_> = hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Sex gap in life expectancy",
                "Life expectancy (HMD, UN WPP)",
                "Life expectancy at birth",
                "School life expectancy in primary education",
            ],
            "an explorer view's name repeats its title, a multi-dimensional \
             view's is null"
        );
        assert_eq!(
            hits[3].url,
            "https://ourworldindata.org/grapher/years-of-schooling?\
             level=primary&metric_type=expected_years_schooling&sex=both"
        );
    }
}
