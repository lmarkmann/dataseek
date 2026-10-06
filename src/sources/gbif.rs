//! GBIF's dataset registry search: occurrence, checklist and sampling-event
//! datasets from biodiversity publishers worldwide.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

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
                title: "Sea-ice meiofauna biodiversity from the Nansen Legacy \
                        cruise Q4 (cruise number: 2019711)"
                    .into(),
                url: "https://www.gbif.org/dataset/\
                      4579d9f1-f913-4d71-8581-e1dd2b812501"
                    .into(),
                description: Some(
                    "The data was collected during the Nansen Legacy seasonal \
                     study (Q4, cruise number: 2019711) from 28.11 - 17.12 \
                     2019 onboard the research vessel RV Kronprins Haakon, \
                     along a transect in the northern Barents Sea from 76N to \
                     82N."
                        .into()
                ),
                publisher: Some("The Nansen Legacy Project".into()),
                doi: Some("10.15468/gx9ujt".into()),
                license: Some(
                    "http://creativecommons.org/licenses/by/4.0/legalcode"
                        .into()
                ),
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
