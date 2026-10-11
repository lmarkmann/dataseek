//! OSF through SHARE's trove index. OSF has no "dataset" resource type, so
//! this searches projects and registrations, which is where OSF data lives.
//! An owner can mark a record's `resourceNature` as Dataset, but 4,135 of
//! more than 10,000 records do, and 2 of the 45 hits for "sea ice", too few
//! to filter on. A page of 100 comes back whole, so one request serves any
//! limit; hits come by relevance and no query syntax draws an error (trove,
//! October 2026). trove throttles per client without saying how: 17 requests
//! in about three minutes drew HTTP 429 with no `Retry-After`, and every
//! `/trove/` path then answered 429 for an hour (October 2026).
//!
//! `acceptMediatype=application/json` is not in trove's OpenAPI, which lists
//! only `application/vnd.api+json` as stable; there the same cards sit under
//! `included[].attributes.resourceMetadata`, reached through the
//! `searchResultPage` relationship (October 2026). `parse` asks for a `data`
//! array so a switch of rendering fails as a changed shape.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

const TYPES: &str =
    "https://osf.io/vocab/2022/Project,https://osf.io/vocab/2022/Registration";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://share.osf.io/trove/index-card-search")
        .query("cardSearchText", query)
        .query("cardSearchFilter[resourceType]", TYPES)
        .query("page[size]", limit.clamp(1, 100))
        .query("acceptMediatype", "application/json")
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if !body.get("data").is_some_and(Value::is_array) {
        return Err(SourceError::shape("no data array"));
    }
    Ok(items(body, "/data").iter().filter_map(record).take(limit).collect())
}

fn record(card: &Value) -> Option<Dataset> {
    let mut dataset =
        Dataset::new(&text(card, "/title/0/@value")?, &text(card, "/@id")?)
            .describe(text(card, "/description/0/@value"))
            .doi_from(
                items(card, "/identifier")
                    .iter()
                    .filter_map(|id| text(id, "/@value"))
                    .find(|id| id.contains("doi.org/")),
            );
    dataset.publisher = text(card, "/publisher/0/name/0/@value");
    dataset.license = text(card, "/rights/0/name/0/@value");
    dataset.updated = day(text(card, "/dateModified/0/@value"));
    dataset.size_bytes = number(card, "/storageByteCount/0/@value");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("osf.json"), 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[1].doi.as_deref(), Some("10.17605/osf.io/zh3w9"));
        assert_eq!(
            hits[0],
            Dataset {
                title: "Climate emotions and pro\u{2011}environmental \
                        behaviour: Associations with well\u{2011}being"
                    .into(),
                url: "https://osf.io/kzt9d".into(),
                description: None,
                publisher: Some("OSF".into()),
                doi: None,
                license: Some("CC-By Attribution 4.0 International".into()),
                updated: Some("2026-06-23".into()),
                size_bytes: Some(142_773),
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_publisher_is_the_service_not_the_person() {
        let hits = parse(&fixture::json("osf.json"), 10).unwrap();
        let publishers: Vec<_> =
            hits.iter().map(|h| h.publisher.as_deref()).collect();
        assert_eq!(
            publishers,
            [Some("OSF"), Some("OSF Registries"), Some("OSF")]
        );
    }

    #[test]
    fn a_data_object_is_a_changed_response_not_an_empty_list() {
        let body = serde_json::json!({"data": {"type": "index-card-search"}});
        assert!(matches!(parse(&body, 10), Err(SourceError::Shape(_))));
    }

    #[test]
    fn the_limit_cuts_the_page_in_the_sources_order() {
        let hits = parse(&fixture::json("osf.json"), 2).unwrap();
        let urls: Vec<_> = hits.iter().map(|h| h.url.as_str()).collect();
        assert_eq!(urls, ["https://osf.io/kzt9d", "https://osf.io/zh3w9"]);
    }
}
