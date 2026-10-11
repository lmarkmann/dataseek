//! CERN Open Data (Invenio), limited to records of type Dataset.
//!
//! Every record carries its whole file list and no parameter trims it: 100
//! records for "AOD" came to 141 MB in 21 s, past the client's body and time
//! limits, and one record alone reached 16 MB. Records are therefore fetched
//! ten to a request and the pages walked with `page` until the limit or the
//! last page. The API answers 60 requests a minute and then 429 with
//! `Retry-After: 60`, and stops at 10,000 results (CERN Open Data, October
//! 2026). A query its parser rejects comes back as HTTP 400 or 500, the 500
//! being what marks a source as down, so it is asked once more with its
//! operators taken out.
//!
//! `updated` is the publication year: the API's own `updated` is the same
//! day for all 100 "AOD" records, a bulk migration on 2025-06-06 (October
//! 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, number, text};

const PAGE: usize = 10;

/// Characters the query parser reads as operators, besides the words AND, OR
/// and NOT in capitals.
const SYNTAX: &[char] = &[
    '+', '-', '=', '&', '|', '<', '>', '!', '(', ')', '{', '}', '[', ']', '^',
    '"', '~', '*', '?', ':', '\\', '/',
];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let size = limit.clamp(1, PAGE);
    let (query, mut body) = first_page(ctx, query, size)?;
    let mut found = parse(&body, limit)?;
    for page in 2..=limit.div_ceil(size) {
        if found.len() >= limit || !has_next(&body) {
            break;
        }
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        body = fetch(ctx, &query, size, page)?;
        found.extend(parse(&body, limit.saturating_sub(found.len()))?);
    }
    Ok(found)
}

fn fetch(
    ctx: &Ctx<'_>,
    query: &str,
    size: usize,
    page: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get("https://opendata.cern.ch/api/records/")
        .query("q", query)
        .query("type", "Dataset")
        .query("size", size)
        .query("page", page)
        .json()
}

/// The first page with the query that got it, which is the plain form when
/// the source rejected the original.
fn first_page(
    ctx: &Ctx<'_>,
    query: &str,
    size: usize,
) -> Result<(String, Value), SourceError> {
    match fetch(ctx, query, size, 1) {
        Ok(body) => Ok((query.to_owned(), body)),
        Err(error) => {
            let plain = plain(query, &error).ok_or(error)?;
            let body = fetch(ctx, &plain, size, 1)?;
            Ok((plain, body))
        }
    }
}

fn plain(query: &str, error: &SourceError) -> Option<String> {
    let rejected = matches!(error, SourceError::Status(400 | 500));
    let plain = query
        .replace(SYNTAX, " ")
        .split_whitespace()
        .map(|word| match word {
            "AND" | "OR" | "NOT" => word.to_lowercase(),
            _ => word.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    (rejected && plain != query && !plain.is_empty()).then_some(plain)
}

fn has_next(body: &Value) -> bool {
    body.pointer("/links/next").is_some()
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

fn record(hit: &Value) -> Option<Dataset> {
    let meta = hit.get("metadata")?;
    let recid = text(meta, "/recid").or_else(|| text(hit, "/id"))?;
    let mut dataset = Dataset::new(
        &text(meta, "/title")?,
        &format!("https://opendata.cern.ch/record/{recid}"),
    )
    .describe(text(meta, "/abstract/description"))
    .doi_from(text(meta, "/doi"));
    dataset.publisher = text(meta, "/experiment/0")
        .or_else(|| text(meta, "/experiment"))
        .map(|e| format!("CERN {e}"));
    dataset.license = text(meta, "/license/attribution");
    dataset.updated = text(meta, "/date_published");
    dataset.size_bytes = number(meta, "/distribution/size");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("cern.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "OPERA muon neutrino event 12316057896".into(),
                url: "https://opendata.cern.ch/record/4803".into(),
                description: Some(
                    "This OPERA muon neutrino event is a muon neutrino \
                     interaction with the lead target where a muon was \
                     reconstructed in the final state. The event data from \
                     Electronic Detectors are available in the Drift Tube, \
                     RPC, and Target Tracker files."
                        .into()
                ),
                publisher: Some("CERN OPERA".into()),
                doi: Some("10.7483/opendata.opera.ocjx.pjsn".into()),
                license: Some("CC0-1.0".into()),
                updated: Some("2018".into()),
                size_bytes: Some(8371),
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn only_a_page_with_a_next_link_is_followed() {
        assert!(has_next(&fixture::json("cern.json")));
        let empty = fixture::json("cern.empty.json");
        assert!(!has_next(&empty));
        assert_eq!(parse(&empty, 10).unwrap(), Vec::<Dataset>::new());
    }

    #[test]
    fn a_rejected_query_is_asked_again_without_its_syntax() {
        for status in [400, 500] {
            assert_eq!(
                plain("\"unbalanced muon", &SourceError::Status(status))
                    .as_deref(),
                Some("unbalanced muon")
            );
        }
        assert_eq!(
            plain("C++ (benchmark", &SourceError::Status(400)).as_deref(),
            Some("C benchmark")
        );
        assert_eq!(
            plain("muon AND", &SourceError::Status(500)).as_deref(),
            Some("muon and")
        );
    }

    #[test]
    fn a_query_is_not_asked_again_when_that_would_change_nothing() {
        assert_eq!(plain("muon neutrino", &SourceError::Status(500)), None);
        assert_eq!(plain("\"\"", &SourceError::Status(400)), None);
        assert_eq!(plain("a:b", &SourceError::Status(404)), None);
        assert_eq!(plain("a:b", &SourceError::RateLimited), None);
    }
}
