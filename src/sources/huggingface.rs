//! Hugging Face Hub dataset search. The Hub has no relevance ranking, and its
//! `search` matches a substring of the repository id, so one word rarely
//! names the dataset: "Iris flower dataset" searched as "dataset" matches
//! thousands of ids, and "climate temperature" as one phrase matches almost
//! none (Hugging Face Hub, October 2026).
//!
//! A query of one meaningful word (see [`crate::catalog::terms`]) asks for
//! `limit` rows sorted by downloads and keeps them as they come. A longer one
//! asks once per anchor, its [`ANCHORS`] longest words, for a page of 1,000
//! sorted by downloads, all at once; it merges the rows by id, counts the
//! query words each row's id, description and tags start a word with, keeps
//! the rows that hold at least half of them and at least two, and orders them
//! by that count, then downloads. Half, because a question's filler words
//! ("how many people live in each US county") match anything two at a time. One page per anchor replaces paging: `skip` answers HTTP 400
//! from 4,000 on. An anchor that fails costs its rows, and the source fails
//! only when every anchor did. Anonymous clients get 500 API requests per IP
//! in 5 minutes (Hugging Face Hub, October 2026).
//!
//! The listing's description is the card's text with each heading alone on a
//! line that starts with two tabs, cut short with "See the full description
//! on the dataset page" and the page's address (Hugging Face Hub, October
//! 2026). The headings and that notice are dropped so the teaser is prose.

use std::collections::HashSet;

use serde_json::Value;

use super::Ctx;
use crate::catalog;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

const PAGE: usize = 1000;
/// How many of a query's words are searched for on the Hub, longest first.
const ANCHORS: usize = 4;
/// The fewest query words a row of a longer query holds, above half of them.
const LEAST_WORDS: usize = 2;
const CUT_NOTICE: &str = " See the full description on the dataset page:";

const FIELDS: [&str; 6] =
    ["author", "description", "downloads", "lastModified", "mainSize", "tags"];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let terms = catalog::terms(query);
    let size = if terms.len() > 1 { PAGE } else { limit };
    let pages: Vec<Result<Value, SourceError>> = std::thread::scope(|scope| {
        let workers: Vec<_> = anchors(&terms)
            .into_iter()
            .map(|anchor| scope.spawn(move || page(ctx, anchor, size)))
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker.join().unwrap_or_else(|_| {
                    Err(SourceError::shape("the adapter crashed"))
                })
            })
            .collect()
    });
    let mut bodies = Vec::new();
    let mut failure = None;
    for page in pages {
        match page {
            Ok(body) => bodies.push(body),
            Err(error) => failure = failure.or(Some(error)),
        }
    }
    match failure {
        Some(error) if bodies.is_empty() => Err(error),
        _ => pick(&bodies, &terms, limit),
    }
}

fn page(
    ctx: &Ctx<'_>,
    anchor: &str,
    size: usize,
) -> Result<Value, SourceError> {
    let mut call = ctx
        .http
        .get("https://huggingface.co/api/datasets")
        .query("search", anchor)
        .query("sort", "downloads")
        .query("limit", size);
    for field in FIELDS {
        call = call.query("expand[]", field);
    }
    if let Some(secret) = ctx.creds.get(Key::HuggingFace) {
        call = call.key_header("Authorization", secret.authorization());
    }
    call.json()
}

/// The [`ANCHORS`] longest distinct terms, the longest first and ties in
/// query order.
fn anchors(terms: &[String]) -> Vec<&str> {
    let mut distinct: Vec<&str> = Vec::new();
    for term in terms {
        if !distinct.contains(&term.as_str()) {
            distinct.push(term);
        }
    }
    distinct.sort_by_key(|term| std::cmp::Reverse(term.len()));
    distinct.truncate(ANCHORS);
    distinct
}

/// One recorded page, as [`pick`] reads it.
#[cfg(test)]
pub(super) fn parse(
    body: &Value,
    terms: &[String],
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    pick(std::slice::from_ref(body), terms, limit)
}

/// The rows of every page, each once: for one term in the Hub's order, for
/// several those holding at least half of them and at least [`LEAST_WORDS`],
/// the most first.
fn pick(
    bodies: &[Value],
    terms: &[String],
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let wide = terms.len() > 1;
    let needles = catalog::needles(terms);
    let least = terms.len().div_ceil(2).max(terms.len().min(LEAST_WORDS));
    let mut seen = HashSet::new();
    let mut scored: Vec<(usize, Dataset)> = Vec::new();
    for body in bodies {
        let rows = body.as_array().ok_or_else(|| {
            SourceError::shape("expected a list of datasets")
        })?;
        for row in rows {
            let Some(id) = text(row, "/id") else { continue };
            if !seen.insert(id) {
                continue;
            }
            let held = if wide { holds(row, &needles) } else { 0 };
            if wide && held < least {
                continue;
            }
            if let Some(dataset) = record(row) {
                scored.push((held, dataset));
            }
        }
    }
    if wide {
        scored.sort_by(|(a, x), (b, y)| {
            b.cmp(a)
                .then_with(|| y.popularity.cmp(&x.popularity))
                .then_with(|| x.title.cmp(&y.title))
        });
    }
    Ok(scored.into_iter().take(limit).map(|(_, dataset)| dataset).collect())
}

/// How many of the query's words start a word of the row's id, description
/// or tags.
fn holds(row: &Value, needles: &[String]) -> usize {
    let bits = catalog::found(&haystack(row), needles).count_ones();
    usize::try_from(bits).unwrap_or(usize::MAX)
}

fn haystack(row: &Value) -> String {
    let mut haystack = text(row, "/id").unwrap_or_default();
    haystack.push(' ');
    haystack.push_str(&text(row, "/description").unwrap_or_default());
    for tag in items(row, "/tags").iter().filter_map(Value::as_str) {
        haystack.push(' ');
        haystack.push_str(tag);
    }
    haystack
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

    /// The rows from `next`'s offset, and a Hub link to the rest while
    /// any are left; the cursor stands in for the Hub's opaque one.
    fn row(id: &str, description: &str, downloads: u64) -> Value {
        serde_json::json!({
            "id": id,
            "author": id.split('/').next(),
            "description": description,
            "downloads": downloads,
            "tags": [],
        })
    }

    fn ids(found: &[Dataset]) -> Vec<&str> {
        found.iter().map(|d| d.title.as_str()).collect()
    }

    #[test]
    fn anchors_are_the_longest_meaningful_words() {
        let terms = catalog::terms("Iris flower dataset");
        assert_eq!(anchors(&terms), ["flower", "iris"]);
        let terms = catalog::terms(
            "GitHub issues and pull requests for training code models",
        );
        assert_eq!(
            anchors(&terms),
            ["requests", "training", "github", "issues"]
        );
        let terms = catalog::terms("iris iris");
        assert_eq!(anchors(&terms), ["iris"]);
    }

    #[test]
    fn rows_from_several_anchors_count_once_and_rank_by_words_held() {
        let terms = words(&["protein", "folding", "benchmark"]);
        let protein = serde_json::json!([
            row("a/protein-folding-benchmark", "", 1),
            row("b/protein-structures", "folding simulations", 900),
            row("c/protein-only", "sequences", 5000),
        ]);
        let benchmark = serde_json::json!([
            row("a/protein-folding-benchmark", "", 1),
            row("d/benchmark-suite", "", 7000),
        ]);
        let found = pick(&[protein, benchmark], &terms, 10).unwrap();
        assert_eq!(
            ids(&found),
            ["a/protein-folding-benchmark", "b/protein-structures"]
        );
    }

    #[test]
    fn a_long_question_needs_half_its_words_not_any_two() {
        let terms = catalog::terms("how many people live in each US county");
        assert_eq!(terms.len(), 7);
        let body = serde_json::json!([
            row("x/how-many-benchmark", "how many", 900),
            row("y/us-county-people", "how many live in each", 10),
        ]);
        assert_eq!(
            ids(&parse(&body, &terms, 10).unwrap()),
            ["y/us-county-people"]
        );
    }

    #[test]
    fn query_words_match_whole_word_starts_only() {
        let terms = words(&["climate", "fund"]);
        let body = serde_json::json!([
            row("x/climate-refund", "a refund policy", 50),
            row("y/climate-funds", "", 10),
        ]);
        assert_eq!(
            ids(&parse(&body, &terms, 10).unwrap()),
            ["y/climate-funds"]
        );
    }

    #[test]
    fn a_one_word_query_keeps_the_hubs_order_unfiltered() {
        let body =
            serde_json::json!(
                [row("x/least", "", 1), row("y/most", "", 900),]
            );
        let found = parse(&body, &words(&["iris"]), 10).unwrap();
        assert_eq!(ids(&found), ["x/least", "y/most"]);
    }
}
