//! Socrata's Discovery API: one endpoint per region that searches every
//! public Socrata domain (US cities and states, data.nasa.gov,
//! healthdata.gov, datos.gov.co, ...), including column names.

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
                    "\u{200b}Monthly residential water consumption grouped \
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
