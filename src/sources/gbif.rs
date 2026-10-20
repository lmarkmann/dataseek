//! GBIF's dataset registry search: occurrence, checklist and sampling-event
//! datasets from biodiversity publishers worldwide. Results come in relevance
//! order, and a page holds up to 1000 datasets, ten times what a search asks
//! for (GBIF, October 2026). Search requests may be answered with HTTP 429
//! when the servers are loaded, at no published rate, and GBIF asks every
//! client to send a User-Agent with a contact address; replies are marked
//! cacheable for ten minutes and no term forbids storing them (GBIF, October
//! 2026). Licenses arrive as three Creative Commons URLs, which become SPDX
//! ids, and as the words "unspecified" and "unsupported" for the 150 datasets
//! with neither, which say nothing and are dropped (GBIF, October 2026).

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
        .get("https://api.gbif.org/v1/dataset/search")
        .query("q", query)
        .query("limit", limit.clamp(1, 1000))
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
    Ok(items(body, "/results").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let key = text(row, "/key")?;
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://www.gbif.org/dataset/{key}"),
    )
    .describe(text(row, "/description"))
    .doi_from(text(row, "/doi"));
    dataset.publisher = text(row, "/publishingOrganizationTitle");
    dataset.license = text(row, "/license");
    dataset.updated = day(text(row, "/modified"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("gbif.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "International Plant Names Index".into(),
                url: "https://www.gbif.org/dataset/\
                      046bbc50-cae2-47ff-aa43-729fbf53f7c5"
                    .into(),
                description: Some(
                    "The International Plant Names Index (IPNI) is a \
                     database of the names and associated basic \
                     bibliographical details of seed plants, ferns and \
                     lycophytes. Its goal is to eliminate the need for \
                     repeated reference to primary sources for basic \
                     bibliographic information about plant names."
                        .into()
                ),
                publisher: Some(
                    "The International Plant Names Index Collaborators".into()
                ),
                doi: Some("10.15468/uhllmw".into()),
                license: Some("unspecified".into()),
                updated: Some("2019-11-11".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn licenses_arrive_as_urls_or_placeholder_words() {
        let hits = parse(&fixture::json("gbif.json"), 10).unwrap();
        let licenses: Vec<_> =
            hits.iter().map(|h| h.license.as_deref()).collect();
        let by = "http://creativecommons.org/licenses/by/4.0/legalcode";
        assert_eq!(
            licenses,
            [
                Some("unspecified"),
                Some(by),
                Some(by),
                Some(
                    "http://creativecommons.org/publicdomain/zero/1.0/legalcode"
                ),
            ]
        );
    }
}
