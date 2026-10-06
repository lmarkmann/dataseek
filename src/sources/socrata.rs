//! Socrata's Discovery API: one endpoint per Socrata cloud that searches every
//! public domain on it, including column names. The US endpoint holds US
//! cities and states and also datos.gov.co; the EU one holds the Catalan,
//! Lombard and Camden portals, among others (Socrata, October 2026).
//!
//! Results come in relevance order. `limit` defaults to 100 and goes up to
//! 10,000 in one request, so a page of up to 100 never needs paging; deeper
//! results need `scroll_id` once `offset + limit` passes 10,000. A request
//! without an app token is throttled per IP address and answers 429 when
//! that trips; dataseek sends no token (Socrata, October 2026).
//!
//! `only=dataset` keeps hosted tabular datasets. External datasets (`href`)
//! and uploaded files (`file`) are left out: many `href` rows only point at
//! a dataset another portal already serves. healthdata.gov lists only `href`
//! rows, so it returns nothing (Socrata, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{base}/api/catalog/v1"))
        .query("q", query)
        .query("only", "dataset")
        .query("limit", limit)
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
    let url = text(row, "/link").or_else(|| text(row, "/permalink"))?;
    let mut dataset = Dataset::new(&text(row, "/resource/name")?, &url)
        .describe(text(row, "/resource/description"));
    dataset.publisher = text(row, "/resource/attribution")
        .or_else(|| text(row, "/metadata/domain"));
    dataset.license = text(row, "/metadata/license");
    dataset.updated = day(text(row, "/resource/updatedAt"));
    dataset.popularity = number(row, "/resource/download_count");
    if let Some(permalink) = text(row, "/permalink") {
        dataset.aliases.push(permalink);
    }
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("socrata.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Austin Water - Residential Water Consumption".into(),
                url: "https://datahub.austintexas.gov/\
                      Utilities-and-City-Services/\
                      Austin-Water-Residential-Water-Consumption/sxk7-7k6z"
                    .into(),
                description: Some(
                    "Monthly residential water consumption grouped \
                     by zip code and customer class."
                        .into()
                ),
                publisher: Some(
                    "City of Austin, Texas - data.austintexas.gov".into()
                ),
                doi: None,
                license: Some("Public Domain".into()),
                updated: Some("2026-03-16".into()),
                size_bytes: None,
                popularity: Some(3841),
                aliases: vec![
                    "https://datahub.austintexas.gov/d/sxk7-7k6z".into()
                ],
            }
        );
    }
}
