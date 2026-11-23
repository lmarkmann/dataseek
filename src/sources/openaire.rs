//! The OpenAIRE Graph, a deduplicated research graph harvested from DataCite,
//! Crossref and thousands of OAI-PMH repositories. Its value next to DataCite
//! is the repositories that never minted a DOI.
//!
//! `/v1` and `/v2` of `researchProducts` are deprecated and "will be removed
//! in the future"; `/v3/research-products` answers the same records in the
//! same order (OpenAIRE, October 2026). A page holds at most 100 records,
//! which is `--per-source`'s ceiling, so a search is one request. The terms
//! allow 60 unauthenticated requests an hour while the response headers
//! report 7,199 (OpenAIRE, October 2026).
//!
//! The query is Solr syntax with uppercase `AND`, `OR` and `NOT`. An
//! unbalanced quote or parenthesis answers 400 and an operator without an
//! operand answers 500, which would park the source as down, so a query it
//! rejects is asked once more as plain words. Phrases and operators that it
//! accepts stay: `"sea ice" AND arctic` found 4,728 records where the same
//! words without syntax found 5,042 (OpenAIRE, October 2026).
//!
//! A record stands for every copy of one product. Its DOIs beyond the first
//! and the landing pages of its copies travel as aliases, ten at most because
//! a PANGAEA series lists 194 DOIs. The API fills no modification date, so
//! the publication date stands in for it, and only the few records with
//! `usageCounts` have a popularity (OpenAIRE, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, items, number, text};

const MAX_ALIASES: usize = 10;

/// What opens or closes a phrase or a group.
const SYNTAX: &[char] = &['"', '(', ')'];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let (_, body) = super::retry_plain(
        query,
        &[400, 500],
        |query| super::plain_query(query, SYNTAX),
        |query| fetch(ctx, query, limit),
    )?;
    parse(&body, limit)
}

fn fetch(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get("https://api.openaire.eu/graph/v3/research-products")
        .query("search", query)
        .query("type", "dataset")
        .query("pageSize", limit.clamp(1, 100))
        .json()
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/results", limit, record)
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dois = items(row, "/pids")
        .iter()
        .filter(|p| p.get("scheme").and_then(Value::as_str) == Some("doi"))
        .filter_map(|p| text(p, "/value").as_deref().and_then(doi));
    let found_doi = dois.next();
    let instance_url = text(row, "/instances/0/urls/0");
    let url = match (&found_doi, &instance_url) {
        (Some(d), _) => format!("https://doi.org/{d}"),
        (None, Some(u)) => u.clone(),
        (None, None) => format!(
            "https://explore.openaire.eu/search/result?id={}",
            text(row, "/id")?
        ),
    };
    let landing_pages = items(row, "/instances")
        .iter()
        .flat_map(|instance| items(instance, "/urls"))
        .filter_map(Value::as_str)
        .filter(|page| !page.contains("doi.org/"))
        .map(str::to_owned);
    let mut aliases: Vec<String> = Vec::new();
    for alias in dois.chain(landing_pages) {
        if alias != url && !aliases.contains(&alias) {
            aliases.push(alias);
        }
        if aliases.len() == MAX_ALIASES {
            break;
        }
    }
    let mut dataset = Dataset::new(&text(row, "/mainTitle")?, &url)
        .describe(text(row, "/descriptions/0"));
    dataset.doi = found_doi;
    dataset.publisher = text(row, "/publisher");
    dataset.license = text(row, "/instances/0/license");
    dataset.updated = day(text(row, "/publicationDate"));
    dataset.popularity = number(row, "/indicators/usageCounts/downloads");
    dataset.aliases = aliases;
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("openaire.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Comparing Block-Based Programming Models for \
                        Two-Armed Robots (supplementary materials)"
                    .into(),
                url: "https://doi.org/10.5281/zenodo.8260344".into(),
                description: Some(
                    "Supplementary Material for the research paper".into()
                ),
                publisher: Some("Zenodo".into()),
                doi: Some("10.5281/zenodo.8260344".into()),
                license: Some("CC BY".into()),
                updated: Some("2020-09-28".into()),
                size_bytes: None,
                popularity: Some(1),
                formats: vec![],
                aliases: vec![
                    "10.5281/zenodo.8260345".into(),
                    "https://zenodo.org/records/8260345".into(),
                ],
            }
        );
    }

    #[test]
    fn a_record_without_counts_publisher_or_license_keeps_its_dois() {
        let hits = parse(&fixture::json("openaire.json"), 10).unwrap();
        assert_eq!(hits[1].doi.as_deref(), Some("10.4225/25/58a3fc093a6c2"));
        assert_eq!(hits[1].aliases, ["10.4225/25/58a443b3dd6c0"]);
        assert_eq!(hits[1].publisher, None);
        assert_eq!(hits[1].license, None);
        assert_eq!(hits[1].popularity, None);
        assert_eq!(hits[3].aliases.len(), 2);
    }

    #[test]
    fn a_zero_download_count_is_a_count() {
        let row = json!({
            "mainTitle": "A table",
            "pids": [{"scheme": "doi", "value": "10.1234/abcd"}],
            "indicators": {"usageCounts": {"downloads": 0, "views": 7}}
        });
        assert_eq!(record(&row).unwrap().popularity, Some(0));
    }

    #[test]
    fn a_series_of_two_hundred_dois_keeps_ten_aliases() {
        let pids: Vec<Value> = (0..200)
            .map(|n| {
                let value = format!("10.1594/pangaea.{n}");
                json!({"scheme": "doi", "value": value})
            })
            .collect();
        let row = json!({"mainTitle": "A series", "pids": pids});
        let dataset = record(&row).unwrap();
        assert_eq!(dataset.doi.as_deref(), Some("10.1594/pangaea.0"));
        assert_eq!(dataset.aliases.len(), MAX_ALIASES);
        assert_eq!(dataset.aliases[0], "10.1594/pangaea.1");
    }

    #[test]
    fn the_plain_form_keeps_every_word_and_drops_the_syntax() {
        for (query, plain) in [
            ("\"unbalanced", "unbalanced"),
            ("C++ (benchmark", "C++ benchmark"),
            ("sea AND", "sea and"),
            ("NOT ice OR", "not ice or"),
            ("sea surface temperature", "sea surface temperature"),
            ("\"()\"", ""),
        ] {
            assert_eq!(super::super::plain_query(query, SYNTAX), plain);
        }
    }
}
