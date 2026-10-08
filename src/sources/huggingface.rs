//! Hugging Face Hub dataset search. Sorted by downloads: the Hub has no
//! relevance ranking, and the most used match is the useful default.
//!
//! The Hub's `search` matches a substring of the repository id, so
//! "climate temperature" finds almost nothing. A multi-word query therefore
//! asks for the longest word and keeps the rows whose id, description or tags
//! contain every word. It reads pages of 1,000, the most the Hub returns, and
//! moves on with `skip` until `limit` rows have passed or five pages are
//! read; a one-word query asks for `limit` rows and stops there. `skip` is
//! not in the Hub's published spec but pages the list, as the Link header's
//! cursor does. Anonymous clients get 500 API requests per IP in 5 minutes
//! (Hugging Face Hub, October 2026).
//!
//! The listing's description is the card's text with each heading alone on a
//! line that starts with two tabs, cut short with "See the full description
//! on the dataset page" and the page's address (Hugging Face Hub, October
//! 2026). The headings and that notice are dropped so the teaser is prose.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

const PAGE: usize = 1000;
const MAX_PAGES: usize = 5;
const CUT_NOTICE: &str = " See the full description on the dataset page:";

const FIELDS: [&str; 6] =
    ["author", "description", "downloads", "lastModified", "mainSize", "tags"];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let words: Vec<String> =
        query.split_whitespace().map(str::to_lowercase).collect();
    let anchor =
        words.iter().max_by_key(|w| w.len()).cloned().unwrap_or_default();
    scan(&words, limit, |skip, size| {
        let mut call = ctx
            .http
            .get("https://huggingface.co/api/datasets")
            .query("search", &anchor)
            .query("sort", "downloads")
            .query("limit", size)
            .query("skip", skip);
        for field in FIELDS {
            call = call.query("expand[]", field);
        }
        if let Some(secret) = ctx.creds.get(Key::HuggingFace) {
            call = call.key_header("Authorization", secret.authorization());
        }
        call.json()
    })
}

fn scan(
    words: &[String],
    limit: usize,
    mut fetch: impl FnMut(usize, usize) -> Result<Value, SourceError>,
) -> Result<Vec<Dataset>, SourceError> {
    let size = if words.len() > 1 { PAGE } else { limit };
    let mut hits = Vec::new();
    for page in 0..MAX_PAGES {
        let body = fetch(page.saturating_mul(size), size)?;
        hits.extend(parse(&body, words, limit.saturating_sub(hits.len()))?);
        let ran_out = body.as_array().map_or(0, Vec::len) < size;
        if ran_out || hits.len() >= limit {
            break;
        }
    }
    Ok(hits)
}

pub(super) fn parse(
    body: &Value,
    words: &[String],
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let wide = words.len() > 1;
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of datasets"))?;
    Ok(rows
        .iter()
        .filter(|row| !wide || mentions_all(row, words))
        .filter_map(record)
        .take(limit)
        .collect())
}

fn mentions_all(row: &Value, words: &[String]) -> bool {
    let mut haystack = text(row, "/id").unwrap_or_default();
    haystack.push(' ');
    haystack.push_str(&text(row, "/description").unwrap_or_default());
    for tag in items(row, "/tags").iter().filter_map(Value::as_str) {
        haystack.push(' ');
        haystack.push_str(tag);
    }
    let haystack = haystack.to_lowercase();
    words.iter().all(|w| haystack.contains(w.as_str()))
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let mut dataset =
        Dataset::new(&id, &format!("https://huggingface.co/datasets/{id}"))
            .describe(prose(row))
            .doi_from(tagged(row, "doi:"));
    dataset.publisher = text(row, "/author");
    dataset.updated = day(text(row, "/lastModified"));
    dataset.size_bytes = number(row, "/mainSize");
    dataset.popularity = number(row, "/downloads");
    dataset.license = tagged(row, "license:");
    dataset.valid()
}

fn tagged(row: &Value, prefix: &str) -> Option<String> {
    items(row, "/tags")
        .iter()
        .filter_map(Value::as_str)
        .find_map(|tag| tag.strip_prefix(prefix))
        .map(str::to_owned)
}

fn prose(row: &Value) -> Option<String> {
    let card = row.get("description")?.as_str()?;
    let (card, _) = card.split_once(CUT_NOTICE).unwrap_or((card, ""));
    let lines: Vec<&str> =
        card.lines().filter(|line| !line.starts_with("\t\t")).collect();
    Some(lines.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    fn words(query: &[&str]) -> Vec<String> {
        query.iter().map(|w| (*w).to_owned()).collect()
    }

    fn recorded_rows() -> Vec<Value> {
        fixture::json("huggingface.json").as_array().unwrap().clone()
    }

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(
            &fixture::json("huggingface.json"),
            &words(&["climate"]),
            10,
        )
        .unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "climatebert/climate_detection".into(),
                url: "https://huggingface.co/datasets/\
                      climatebert/climate_detection"
                    .into(),
                description: Some(
                    "We introduce an expert-annotated dataset for detecting \
                     climate-related paragraphs in corporate disclosures. \
                     The dataset supports a binary classification task of \
                     whether a given paragraph is climate-related or not. \
                     The text in the dataset is in English. { 'text': \
                     '\u{2212} Scope 3: Optional scope that includes\u{2026}"
                        .into()
                ),
                publisher: Some("climatebert".into()),
                doi: None,
                license: Some("cc-by-nc-sa-4.0".into()),
                updated: Some("2023-04-18".into()),
                size_bytes: Some(498_654),
                popularity: Some(254),
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[1].description.as_deref(),
            Some(
                "This repository contains a dataset based on funding \
                 proposals of 21 climate mitigation projects, submitted to \
                 the Green Climate Fund (GCF)."
            )
        );
        assert_eq!(hits[1].doi.as_deref(), Some("10.57967/hf/9046"));
        assert_eq!(hits[1].license.as_deref(), Some("mpl-2.0"));
        assert_eq!(hits[2].description, None);
        assert_eq!(hits[2].license, None);
    }

    #[test]
    fn a_query_with_several_words_keeps_rows_that_mention_all_of_them() {
        let hits = parse(
            &fixture::json("huggingface.json"),
            &words(&["climate", "fund"]),
            10,
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].url,
            "https://huggingface.co/datasets/JavierSanzCruza/ClimateFund"
        );
    }

    fn page(rows: &[Value], skip: usize, size: usize) -> Value {
        Value::Array(rows.iter().skip(skip).take(size).cloned().collect())
    }

    #[test]
    fn a_one_word_query_asks_for_exactly_limit_rows_and_stops() {
        let rows = recorded_rows();
        let mut asked = Vec::new();
        let hits = scan(&words(&["climate"]), 2, |skip, size| {
            asked.push((skip, size));
            Ok(page(&rows, skip, size))
        })
        .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(asked, [(0, 2)]);
    }

    #[test]
    fn a_short_page_means_the_hub_has_no_more() {
        let rows = recorded_rows();
        let mut asked = Vec::new();
        let hits = scan(&words(&["climate"]), 10, |skip, size| {
            asked.push((skip, size));
            Ok(page(&rows, skip, size))
        })
        .unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(asked, [(0, 10)]);
    }

    #[test]
    fn a_wide_query_reads_on_with_skip_until_enough_rows_match() {
        let recorded = recorded_rows();
        let mut stream = vec![recorded[0].clone(); PAGE];
        stream.extend(recorded);
        let mut asked = Vec::new();
        let hits = scan(&words(&["climate", "fund"]), 5, |skip, size| {
            asked.push((skip, size));
            Ok(page(&stream, skip, size))
        })
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(asked, [(0, PAGE), (PAGE, PAGE)]);
    }

    #[test]
    fn a_wide_query_gives_up_after_five_pages() {
        let stream = vec![recorded_rows()[0].clone(); PAGE * (MAX_PAGES + 1)];
        let mut asked = 0;
        let hits = scan(&words(&["climate", "fund"]), 5, |skip, size| {
            asked += 1;
            Ok(page(&stream, skip, size))
        })
        .unwrap();
        assert_eq!(hits.len(), 0);
        assert_eq!(asked, MAX_PAGES);
    }
}
