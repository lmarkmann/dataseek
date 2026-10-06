//! Data.gov's catalog. With a key (api.data.gov) it calls the documented
//! Catalog API on api.gsa.gov; without one it asks catalog.data.gov's own
//! search, which returns the same JSON keyless. Both replaced the CKAN API,
//! which data.gov retired in 2025.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, from_markdown, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let call = match ctx.creds.get(Key::DataGov) {
        Some(secret) => ctx
            .http
            .get("https://api.gsa.gov/technology/datagov/v4/search")
            .header("X-Api-Key", secret.token()),
        None => ctx.http.get("https://catalog.data.gov/search"),
    };
    let body = call.query("q", query).query("per_page", limit).json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let rows = body
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| SourceError::shape("no results array"))?;
    Ok(rows.iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let slug = text(row, "/slug")?;
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://catalog.data.gov/dataset/{slug}"),
    )
    .describe(text(row, "/description").map(|d| from_markdown(&d)))
    .doi_from(text(row, "/dcat/identifier"));
    dataset.publisher = text(row, "/dcat/publisher/name")
        .or_else(|| text(row, "/organization/name"));
    dataset.license = text(row, "/dcat/license");
    dataset.updated = day(text(row, "/dcat/modified"));
    if let Some(landing) = text(row, "/dcat/landingPage") {
        dataset.aliases.push(landing);
    }
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("datagov.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[3].description.as_deref(),
            Some(
                "NOTE: This dataset is no longer being updated. For current \
                 water supply and conservation data, please use this \
                 dataset: Urban Retail Water Supplier - Water Conservation, \
                 Supply, and Demand (June 2014 onwards)"
            )
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: "Drinking Water - Public Water System Annually \
                        Reported Water Production and Delivery Information \
                        2013-2022"
                    .into(),
                url: "https://catalog.data.gov/dataset/\
                      drinking-water-public-water-system-annually-reported-\
                      water-production-and-delive-2013-2022"
                    .into(),
                description: Some(
                    "Amount of water produced by month and by source and \
                     the water delivered by type of use and by month for \
                     every Public Water System (PWS) reporting. Public Water \
                     Systems submit their annual inventory information \
                     using the electronic Annual Report (eAR) submission \
                     process."
                        .into()
                ),
                publisher: Some(
                    "California State Water Resources Control Board".into()
                ),
                doi: None,
                license: Some(
                    "http://www.opendefinition.org/licenses/cc-by".into()
                ),
                updated: Some("2024-11-27".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
