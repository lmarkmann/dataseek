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
//! ("how many people live in each US county") match anything two at a time.
//! One page per anchor replaces paging: `skip` answers HTTP 400 from 4,000
//! on. A failed anchor fails the search, so a list missing an anchor's rows
//! is never cached as the answer. Anonymous clients get 500 API requests per
//! IP in 5 minutes (Hugging Face Hub, October 2026).
//!
//! A longer query also asks the Hub's full-text search,
//! `/api/search/full-text` with `type=dataset`, which matches the text of the
//! dataset cards: "sea surface temperature" is in no repository id and in
//! 1,145 cards (probed 2026-11-23). One request returns at most 100 hits in
//! the Hub's own order, and `q` holds at most 250 characters (the Hub's
//! OpenAPI file). A hit carries the id, the owner, the update time in
//! milliseconds, the tags as one comma-joined string, and the card's text
//! from its first line on, as fragments with each matched word apart. The
//! start of that text, read as Markdown, is the description. A hit has no
//! size or download count: its `likes` are not downloads. A dataset the
//! full-text search finds passes the word filter, because the Hub matched
//! its card; it counts the query words in its id, tags and description.
//! Where an anchor page holds the same id, that row is the record: its
//! description is the Hub's own teaser. The word count still orders the
//! list, then the downloads, then the Hub's full-text order: with the
//! full-text order first, nDCG@10 on the tuning queries of `just relevance`
//! fell from 0.646 to 0.595 (2026-11-23). A failed full-text call fails the
//! search like a failed anchor. The full-text search has its own limit, 50
//! requests per IP in 5 minutes, and a token does not raise it (response
//! headers, 2026-11-23).
//!
//! The listing's description is the card's text with each heading alone on a
//! line that starts with two tabs, cut short with "See the full description
//! on the dataset page" and the page's address (Hugging Face Hub, October
//! 2026). The headings and that notice are dropped so the teaser is prose.

use std::collections::{HashMap, HashSet};
use std::thread::ScopedJoinHandle;

use serde_json::Value;

use super::Ctx;
use crate::catalog;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{
    Dataset, date_from_epoch, day, from_markdown, items, number, text,
};

const PAGE: usize = 1000;
/// How many of a query's words are searched for on the Hub, longest first.
const ANCHORS: usize = 4;
/// The fewest query words a row of a longer query holds, above half of them.
const LEAST_WORDS: usize = 2;
/// The most hits one full-text request returns.
const HITS: usize = 100;
/// The longest query the full-text search takes, in characters.
const LONGEST_QUERY: usize = 250;
/// How many characters of a hit's card are read for its description. A card
/// can hold 850,000, and the description shows the first 320 of its prose.
const CARD_START: usize = 4000;
const CUT_NOTICE: &str = " See the full description on the dataset page:";

/// The fields a listing row is asked to carry.
pub const FIELDS: [&str; 6] =
    ["author", "description", "downloads", "lastModified", "mainSize", "tags"];

type Fetched = Result<Value, SourceError>;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let terms = catalog::terms(query);
    let wide = terms.len() > 1;
    let size = if wide { PAGE } else { limit };
    let (pages, hits) = std::thread::scope(|scope| {
        let cards = wide.then(|| scope.spawn(move || full_text(ctx, query)));
        let workers: Vec<_> = anchors(&terms)
            .into_iter()
            .map(|anchor| scope.spawn(move || page(ctx, anchor, size)))
            .collect();
        let pages: Vec<Fetched> = workers.into_iter().map(joined).collect();
        (pages, cards.map(joined))
    });
    settle(pages, hits, &terms, limit)
}

fn joined(worker: ScopedJoinHandle<'_, Fetched>) -> Fetched {
    worker
        .join()
        .unwrap_or_else(|_| Err(SourceError::shape("the adapter crashed")))
}

/// The anchors' pages and the full-text hits as one answer, or the first
/// failure among them.
fn settle(
    pages: Vec<Fetched>,
    hits: Option<Fetched>,
    terms: &[String],
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let bodies = pages.into_iter().collect::<Result<Vec<_>, _>>()?;
    let hits = hits.transpose()?;
    pick(&bodies, hits.as_ref(), terms, limit)
}

fn full_text(ctx: &Ctx<'_>, query: &str) -> Fetched {
    let cut: String = query.chars().take(LONGEST_QUERY).collect();
    let mut call = ctx
        .http
        .get("https://huggingface.co/api/search/full-text")
        .query("q", cut)
        .query("type", "dataset")
        .query("limit", HITS);
    if let Some(secret) = ctx.creds.get(Key::HuggingFace) {
        call = call.key_header("Authorization", secret.authorization());
    }
    call.json()
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
    pick(std::slice::from_ref(body), None, terms, limit)
}

/// One recorded full-text answer, as [`pick`] reads it for a longer query.
#[cfg(test)]
pub(super) fn parse_hits(
    body: &Value,
    terms: &[String],
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    pick(&[], Some(body), terms, limit)
}

/// A row to rank: the query words it holds, its place among the full-text
/// hits, and the record.
type Scored = (usize, Option<usize>, Dataset);

/// The rows of every page and the full-text hits, each dataset once: for one
/// term in the Hub's order, for several those holding at least half of them
/// and at least [`LEAST_WORDS`] or found by the full-text search, the most
/// words first.
fn pick(
    bodies: &[Value],
    hits: Option<&Value>,
    terms: &[String],
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let needles = catalog::needles(terms);
    let least = (terms.len() > 1)
        .then(|| terms.len().div_ceil(2).max(terms.len().min(LEAST_WORDS)));
    let hits = match hits {
        Some(body) => hit_list(body)?,
        None => &[],
    };
    let mut ranks: HashMap<String, usize> = HashMap::new();
    for (rank, hit) in hits.iter().enumerate() {
        if let Some(name) = text(hit, "/name") {
            ranks.entry(name).or_insert(rank);
        }
    }
    let mut seen = HashSet::new();
    let mut scored: Vec<Scored> = Vec::new();
    for body in bodies {
        let rows = body.as_array().ok_or_else(|| {
            SourceError::shape("expected a list of datasets")
        })?;
        scored.extend(rows.iter().filter_map(|row| {
            listed(row, &ranks, &mut seen, &needles, least)
        }));
    }
    scored.extend(
        hits.iter()
            .enumerate()
            .filter_map(|(rank, hit)| found(hit, rank, &mut seen, &needles)),
    );
    if least.is_some() {
        scored.sort_by(order);
    }
    Ok(scored.into_iter().take(limit).map(|(_, _, dataset)| dataset).collect())
}

/// A listing row to rank, the first time its dataset shows. For a query of
/// several words `least` is set, and a row that holds fewer of them stays
/// only when the full-text search found its dataset too.
fn listed(
    row: &Value,
    ranks: &HashMap<String, usize>,
    seen: &mut HashSet<String>,
    needles: &[String],
    least: Option<usize>,
) -> Option<Scored> {
    let id = text(row, "/id")?;
    let rank = ranks.get(&id).copied();
    if !seen.insert(id) {
        return None;
    }
    let held = least.map_or(0, |_| holds(&haystack(row), needles));
    if least.is_some_and(|least| held < least) && rank.is_none() {
        return None;
    }
    Some((held, rank, record(row)?))
}

/// A full-text hit to rank, when no listing row showed its dataset.
fn found(
    hit: &Value,
    rank: usize,
    seen: &mut HashSet<String>,
    needles: &[String],
) -> Option<Scored> {
    if !seen.insert(text(hit, "/name")?) {
        return None;
    }
    let dataset = hit_record(hit)?;
    let held = holds(&hit_haystack(hit, &dataset), needles);
    Some((held, Some(rank), dataset))
}

/// The most query words first, then the most downloads, then the full-text
/// place, then the title.
fn order(one: &Scored, other: &Scored) -> std::cmp::Ordering {
    let (held, place, dataset) = one;
    let (other_held, other_place, other_dataset) = other;
    other_held
        .cmp(held)
        .then_with(|| other_dataset.popularity.cmp(&dataset.popularity))
        .then_with(|| {
            place.unwrap_or(usize::MAX).cmp(&other_place.unwrap_or(usize::MAX))
        })
        .then_with(|| dataset.title.cmp(&other_dataset.title))
}

/// The hits of a full-text answer.
fn hit_list(body: &Value) -> Result<&[Value], SourceError> {
    body.get("hits")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| SourceError::shape("no hits list"))
}

/// How many of the query's words start a word of `haystack`.
fn holds(haystack: &str, needles: &[String]) -> usize {
    let bits = catalog::found(haystack, needles).count_ones();
    usize::try_from(bits).unwrap_or(usize::MAX)
}

/// A listing row's id, description and tags.
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

/// A full-text hit's id, tags and description.
fn hit_haystack(hit: &Value, dataset: &Dataset) -> String {
    let mut haystack = dataset.title.clone();
    haystack.push(' ');
    haystack.push_str(&text(hit, "/tags").unwrap_or_default());
    haystack.push(' ');
    haystack.push_str(dataset.description.as_deref().unwrap_or_default());
    haystack
}

fn hit_record(hit: &Value) -> Option<Dataset> {
    let name = text(hit, "/name")?;
    let tags = text(hit, "/tags").unwrap_or_default();
    let tagged = |prefix: &str| {
        tags.split(',')
            .find_map(|tag| tag.trim().strip_prefix(prefix))
            .map(str::to_owned)
    };
    let mut dataset = Dataset::new(
        &name,
        &format!("https://huggingface.co/datasets/{name}"),
    )
    .describe(card_start(hit))
    .doi_from(tagged("doi:"));
    dataset.publisher = text(hit, "/repoOwner");
    dataset.updated = number(hit, "/updatedAt").and_then(date_from_epoch);
    dataset.license = tagged("license:");
    dataset.valid()
}

/// The start of a hit's card as prose. The Hub returns the card as
/// fragments, each matched word apart from the text around it.
fn card_start(hit: &Value) -> Option<String> {
    let card: String = items(hit, "/formatted/fileContent")
        .iter()
        .filter_map(|fragment| fragment.get("text")?.as_str())
        .flat_map(str::chars)
        .take(CARD_START)
        .collect();
    (!card.is_empty()).then(|| from_markdown(&card))
}

pub(super) fn record(row: &Value) -> Option<Dataset> {
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
                formats: vec![],
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
        let found = pick(&[protein, benchmark], None, &terms, 10).unwrap();
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
    fn one_failed_anchor_fails_the_search() {
        let terms = words(&["climate", "fund"]);
        let found = serde_json::json!([row("y/climate-funds", "", 10)]);
        let pages = vec![Ok(found), Err(SourceError::RateLimited)];
        let outcome = settle(pages, None, &terms, 10);
        assert!(matches!(outcome, Err(SourceError::RateLimited)));
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

    const SST: [&str; 3] = ["sea", "surface", "temperature"];

    #[test]
    fn full_text_hits_map_from_a_recorded_search() {
        let hits = parse_hits(
            &fixture::json("huggingface.fulltext.json"),
            &words(&SST),
            10,
        )
        .unwrap();
        assert_eq!(
            ids(&hits),
            [
                "PranavKonijeti/viirs-sst-daily-nonNRT",
                "PranavKonijeti/viirs-sst-daily-nrt",
                "nilsleh/OceanTACO",
                "louisbrulenaudet/obis",
                "edilia12380/era5-daily-weather-western-europe",
            ]
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: "PranavKonijeti/viirs-sst-daily-nonNRT".into(),
                url: "https://huggingface.co/datasets/\
                      PranavKonijeti/viirs-sst-daily-nonNRT"
                    .into(),
                description: Some(
                    "VIIRS/SNPP Daily Sea Surface Temperature \u{2014} 4 km, \
                     Science Quality Daily global sea surface temperature \
                     from the VIIRS instrument aboard Suomi-NPP, Level-3 \
                     Standard Mapped Image at 4 km, as distributed by the \
                     NASA Ocean Biology Processing Group. This is the \
                     science-quality (refined) feed \u{2014} fully \
                     calibrated and..."
                        .into()
                ),
                publisher: Some("PranavKonijeti".into()),
                doi: None,
                license: Some("other".into()),
                updated: Some("2026-07-31".into()),
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
        assert_eq!(hits[2].doi.as_deref(), Some("10.57967/hf/8171"));
        assert_eq!(hits[2].license.as_deref(), Some("cc-by-4.0"));
        assert_eq!(hits[2].updated.as_deref(), Some("2026-09-08"));
    }

    fn hit(name: &str, tags: &str) -> Value {
        serde_json::json!({
            "name": name,
            "repoOwner": name.split('/').next(),
            "tags": tags,
            "updatedAt": 1_765_224_169_324_u64,
        })
    }

    #[test]
    fn a_dataset_in_both_answers_appears_once_as_its_listing_row() {
        let page = serde_json::json!([row(
            "a/sst-fields",
            "sea surface temperature fields",
            40
        )]);
        let cards = serde_json::json!({"hits": [
            hit("a/sst-fields", "region:us"),
            hit("b/ocean-cards", "region:us"),
        ]});
        let found = pick(&[page], Some(&cards), &words(&SST), 10).unwrap();
        assert_eq!(ids(&found), ["a/sst-fields", "b/ocean-cards"]);
        assert_eq!(found[0].popularity, Some(40));
        assert_eq!(
            found[0].description.as_deref(),
            Some("sea surface temperature fields")
        );
    }

    #[test]
    fn a_dataset_found_by_full_text_passes_the_word_filter() {
        let page = serde_json::json!([
            row("c/buoys", "moored buoy records", 7),
            row("d/sea-ice", "ice extent", 9000),
        ]);
        let cards = serde_json::json!({"hits": [
            hit("b/ocean-cards", "region:us"),
            hit("c/buoys", "region:us"),
        ]});
        let found = pick(&[page], Some(&cards), &words(&SST), 10).unwrap();
        assert_eq!(ids(&found), ["c/buoys", "b/ocean-cards"]);
        assert_eq!(found[0].popularity, Some(7));
        assert_eq!(found[1].updated.as_deref(), Some("2025-12-08"));
    }

    #[test]
    fn the_words_a_dataset_holds_outrank_its_full_text_place() {
        let page = serde_json::json!([row(
            "a/sst-fields",
            "sea surface temperature fields",
            40
        )]);
        let cards = serde_json::json!({"hits": [
            hit("b/ocean-cards", "region:us"),
            hit("e/sea-temperature", "region:us"),
            hit("f/argo-floats", "region:us"),
        ]});
        let found = pick(&[page], Some(&cards), &words(&SST), 10).unwrap();
        assert_eq!(
            ids(&found),
            [
                "a/sst-fields",
                "e/sea-temperature",
                "b/ocean-cards",
                "f/argo-floats",
            ]
        );
    }

    #[test]
    fn a_failed_full_text_call_fails_the_search() {
        let page = serde_json::json!([row("y/climate-funds", "", 10)]);
        let outcome = settle(
            vec![Ok(page)],
            Some(Err(SourceError::RateLimited)),
            &words(&["climate", "fund"]),
            10,
        );
        assert!(matches!(outcome, Err(SourceError::RateLimited)));
    }
}
