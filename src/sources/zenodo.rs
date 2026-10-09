//! Zenodo's records search (InvenioRDM), limited to resource type dataset.
//! Anonymous pages are capped at 25 (a larger `size` answers HTTP 400) and
//! the search endpoints at 30 requests a minute (Zenodo, October 2026).
//! `--per-source` is at most 100, so a search reads up to four pages in the
//! source's order and stops early when `links.next` is gone. The query is
//! Lucene syntax, and one it cannot parse ("sea/ice", a lone "!", "a &&")
//! answers HTTP 500, which would park the source as down, so the search is
//! repeated once with every operator character escaped (Zenodo, October
//! 2026). The concept DOI travels as an alias so versions of one record merge.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

const PAGE: usize = 25;
const OPERATORS: &str = r#"+-=&|><!(){}[]^"~*?:\/"#;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    with_plain_retry(query, |query| {
        pages(limit, |size, page| {
            if ctx.stopped() {
                return Err(SourceError::Stopped);
            }
            ctx.http
                .get("https://zenodo.org/api/records")
                .query("q", query)
                .query("type", "dataset")
                .query("size", size)
                .query("page", page)
                .json()
        })
    })
}

/// Phrases and wildcards work as typed, so the query goes out as written
/// first; only an HTTP 500 repeats it once with every operator escaped.
fn with_plain_retry(
    query: &str,
    run: impl Fn(&str) -> Result<Vec<Dataset>, SourceError>,
) -> Result<Vec<Dataset>, SourceError> {
    match run(query) {
        Err(SourceError::Status(500)) => {
            let plain = plain_words(query);
            if plain == query {
                Err(SourceError::Status(500))
            } else {
                run(&plain)
            }
        }
        found => found,
    }
}

/// Pages of one size, fetched in order until `limit` records are in hand or
/// the source has no next page. A failed page fails the search, so a cut-short
/// list is never cached as the answer for this `limit`.
fn pages(
    limit: usize,
    mut fetch: impl FnMut(usize, usize) -> Result<Value, SourceError>,
) -> Result<Vec<Dataset>, SourceError> {
    let size = limit.clamp(1, PAGE);
    let mut found = Vec::new();
    for page in 1..=limit.div_ceil(size) {
        let body = fetch(size, page)?;
        found.extend(parse(&body, limit.saturating_sub(found.len()))?);
        if found.len() >= limit || body.pointer("/links/next").is_none() {
            break;
        }
    }
    Ok(found)
}

fn plain_words(query: &str) -> String {
    let mut plain = String::with_capacity(query.len());
    for c in query.chars() {
        if OPERATORS.contains(c) {
            plain.push('\\');
        }
        plain.push(c);
    }
    plain
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/hits/hits").is_none() {
        return Err(SourceError::shape("no hits.hits"));
    }
    Ok(items(body, "/hits/hits")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let meta = row.get("metadata")?;
    let url = text(row, "/links/self_html").or_else(|| {
        text(row, "/id").map(|id| format!("https://zenodo.org/records/{id}"))
    })?;
    let mut dataset = Dataset::new(&text(meta, "/title")?, &url)
        .describe(text(meta, "/description"))
        .doi_from(text(row, "/doi"));
    dataset.publisher = text(meta, "/creators/0/name");
    dataset.license = text(meta, "/license/id");
    dataset.updated = day(text(row, "/updated"));
    let sizes: Vec<u64> = items(row, "/files")
        .iter()
        .filter_map(|f| number(f, "/size"))
        .collect();
    if !sizes.is_empty() {
        dataset.size_bytes =
            Some(sizes.iter().fold(0, |a, b| a.saturating_add(*b)));
    }
    dataset.popularity = number(row, "/stats/unique_downloads");
    if let Some(concept) = text(row, "/conceptdoi") {
        dataset.aliases.push(concept);
    }
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("zenodo.json"), 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Monthly water storage levels, Victoria".into(),
                url: "https://zenodo.org/records/23077368".into(),
                description: Some(
                    "The volume held in each of about 65 Victorian reservoirs \
                     at the end of every month since January 2010, in \
                     megalitres, one row per reservoir and month. Updated \
                     monthly."
                        .into()
                ),
                publisher: Some(
                    "Department of Energy, Environment and Climate Action"
                        .into()
                ),
                doi: Some("10.5281/zenodo.23077368".into()),
                license: Some("cc-by-4.0".into()),
                updated: Some("2026-10-01".into()),
                size_bytes: Some(585 + 939_174 + 242_821),
                popularity: Some(3),
                aliases: vec!["10.5281/zenodo.23077367".into()],
            }
        );
    }

    fn full_page() -> Value {
        let mut body = fixture::json("zenodo.json");
        let hit = body.pointer("/hits/hits/0").unwrap().clone();
        *body.pointer_mut("/hits/hits").unwrap() =
            Value::Array(vec![hit; PAGE]);
        body
    }

    #[test]
    fn pages_of_25_are_read_until_the_limit() {
        let mut asked = Vec::new();
        let hits = pages(60, |size, page| {
            asked.push((size, page));
            Ok(full_page())
        })
        .unwrap();
        assert_eq!(asked, [(25, 1), (25, 2), (25, 3)]);
        assert_eq!(hits.len(), 60);
    }

    #[test]
    fn a_small_limit_asks_for_one_small_page() {
        let mut asked = Vec::new();
        let hits = pages(2, |size, page| {
            asked.push((size, page));
            Ok(full_page())
        })
        .unwrap();
        assert_eq!(asked, [(2, 1)]);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn the_last_page_ends_the_search() {
        let mut body = full_page();
        body.as_object_mut().unwrap().remove("links");
        let mut asked = 0;
        let hits = pages(100, |_, _| {
            asked += 1;
            Ok(body.clone())
        })
        .unwrap();
        assert_eq!((asked, hits.len()), (1, 25));
    }

    #[test]
    fn a_failed_page_fails_the_search() {
        let result = pages(100, |_, page| {
            if page == 1 {
                Ok(full_page())
            } else {
                Err(SourceError::RateLimited)
            }
        });
        assert!(matches!(result, Err(SourceError::RateLimited)));
    }

    #[test]
    fn operator_characters_are_escaped() {
        for (query, plain) in [
            ("sea ice", "sea ice"),
            ("sea/ice", r"sea\/ice"),
            ("10.5281/zenodo.1", r"10.5281\/zenodo.1"),
            ("a !", r"a \!"),
            ("a\"b", r#"a\"b"#),
            ("a &&", r"a \&\&"),
        ] {
            assert_eq!(plain_words(query), plain, "{query}");
        }
    }

    #[test]
    fn a_query_is_escaped_only_after_zenodo_answers_500() {
        let asked = std::cell::RefCell::new(Vec::new());
        let run = |query: &str| {
            asked.borrow_mut().push(query.to_owned());
            if query.contains('\\') {
                Ok(Vec::new())
            } else {
                Err(SourceError::Status(500))
            }
        };
        assert!(with_plain_retry("sea/ice", run).is_ok());
        assert_eq!(*asked.borrow(), ["sea/ice", r"sea\/ice"]);

        asked.borrow_mut().clear();
        let down = |query: &str| {
            asked.borrow_mut().push(query.to_owned());
            Err(SourceError::Status(500))
        };
        assert!(with_plain_retry("sea ice", down).is_err());
        assert_eq!(asked.borrow().len(), 1, "nothing to escape, no retry");

        asked.borrow_mut().clear();
        let fine = |query: &str| {
            asked.borrow_mut().push(query.to_owned());
            Ok(Vec::new())
        };
        assert!(with_plain_retry("\"sea ice\"", fine).is_ok());
        assert_eq!(
            *asked.borrow(),
            ["\"sea ice\""],
            "a phrase goes out as typed"
        );
    }
}
