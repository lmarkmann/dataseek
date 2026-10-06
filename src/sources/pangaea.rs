//! PANGAEA's search service, the JSON behind the search box on pangaea.de.
//! PANGAEA calls this API internal and undocumented while an official search
//! API is under development, and its robots.txt disallows `/advanced/`
//! (PANGAEA, October 2026). Its JSON carries each hit's citation as an HTML
//! fragment, `<strong>Author (year):</strong> Title</a>`, which is where the
//! title comes from. The authors have no field in a [`Dataset`] and are not
//! the publisher: DataCite names PANGAEA on 439,586 of its 439,773 DOIs
//! (October 2026).
//!
//! A page holds at most 500 hits and `count` is capped there, so one request
//! covers any `--per-source` (PANGAEA, October 2026). Hits come by relevance
//! score and every word must match. The fragment has no license or date, and its
//! size is a count of data points or datasets, not bytes. Seven in ten hits
//! have no abstract because PANGAEA holds none for older datasets (285 of
//! 400 hits over four queries, October 2026).
//!
//! A query the service cannot parse (`ocean: temperature`, an unclosed quote
//! or bracket, a leading AND, a word that starts with `*` or `?`) answers
//! HTTP 500, which would park the source as down, so it is asked once more
//! as plain words (October 2026). The terms
//! allow PANGAEA to cut off clients whose requests "significantly" exceed the
//! average of its other users, with no number given.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, doi, items, text};

const COUNT_CAP: usize = 500;

/// Characters that open or close a field prefix, a phrase or a group.
const SYNTAX: &[char] = &[':', '"', '(', ')', '[', ']', '{', '}'];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let mut body = request(ctx, query, limit);
    if matches!(body, Err(SourceError::Status(500))) {
        let plain = plain_words(query);
        if !plain.is_empty() && plain != query {
            body = request(ctx, &plain, limit);
        }
    }
    parse(&body?, limit)
}

fn request(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get("https://www.pangaea.de/advanced/search.php")
        .query("q", query)
        .query("count", limit.clamp(1, COUNT_CAP))
        .json()
}

/// The words of a query without the characters that make PANGAEA read a
/// field prefix, phrase or group, without a wildcard that opens a word, and
/// without the bare AND, OR and NOT.
fn plain_words(query: &str) -> String {
    query
        .split(|c: char| c.is_whitespace() || SYNTAX.contains(&c))
        .map(|word| word.trim_start_matches(['*', '?']))
        .filter(|word| !word.is_empty())
        .filter(|word| !matches!(*word, "AND" | "OR" | "NOT"))
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
    let rows = items(body, "/results");
    let found: Vec<Dataset> =
        rows.iter().filter_map(record).take(limit).collect();
    if found.is_empty() && !rows.is_empty() {
        return Err(SourceError::shape("no citation in any result"));
    }
    Ok(found)
}

fn record(row: &Value) -> Option<Dataset> {
    let found = text(row, "/URI").as_deref().and_then(doi)?;
    let html = text(row, "/html")?;
    let title = html.split_once("</strong>")?.1.split_once("</a>")?.0;
    let abstract_text = html
        .split_once("Abstract:")
        .and_then(|(_, tail)| tail.split_once("<td class=\"content\">"))
        .and_then(|(_, tail)| tail.split_once("</td>"))
        .map(|(inner, _)| inner.to_owned());
    let mut dataset = Dataset::new(title, &format!("https://doi.org/{found}"))
        .describe(abstract_text);
    dataset.doi = Some(found);
    dataset.publisher = Some("PANGAEA".to_owned());
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("pangaea.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Sea ice thickness and sea ice area transport in the \
                        Laptev Sea"
                    .into(),
                url: "https://doi.org/10.1594/pangaea.880357".into(),
                description: Some(
                    "Recent studies based on satellite observations have \
                     shown that there is a high statistical connection \
                     between the late winter (Feb-May) sea ice export out the \
                     Laptev Sea, and the ice coverage in the following summer."
                        .into()
                ),
                publisher: Some("PANGAEA".into()),
                doi: Some("10.1594/pangaea.880357".into()),
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_changed_citation_is_a_shape_error_not_an_empty_list() {
        let mut body = fixture::json("pangaea.json");
        for row in body["results"].as_array_mut().unwrap() {
            row["html"] = json!("<li>Sea ice, Laptev Sea</li>");
        }
        assert!(matches!(parse(&body, 10), Err(SourceError::Shape(_))));
        assert!(
            parse(&json!({"results": []}), 10).unwrap().is_empty(),
            "a search with no hits is an answer"
        );
    }

    #[test]
    fn plain_words_drop_what_the_parser_rejected() {
        for (query, plain) in [
            ("ocean: temperature", "ocean temperature"),
            ("\"unbalanced quote", "unbalanced quote"),
            ("foo/bar (x", "foo/bar x"),
            ("AND sea ice", "sea ice"),
            ("a:b", "a b"),
            ("sea *ice", "sea ice"),
            ("?sea se*a", "sea se*a"),
        ] {
            assert_eq!(plain_words(query), plain, "{query}");
        }
    }
}
