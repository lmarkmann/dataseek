//! Data.gov's catalog. With a key (api.data.gov) it calls the documented
//! Catalog API on api.gsa.gov; without one it asks catalog.data.gov's own
//! search, which serves the same JSON and is described by the OpenAPI file at
//! catalog.data.gov/openapi.json. Both replaced the CKAN API, which answers
//! 404 on catalog.data.gov (Data.gov, October 2026).
//!
//! `per_page` takes 1 to 1000 (Data.gov OpenAPI, October 2026), so one page
//! covers any `limit`. A personal key allows 1,000 requests an hour; the
//! keyless host states no limit (api.data.gov, October 2026). `popularity` is
//! the dataset page's views in the last month, the figure the catalog prints
//! as "Views last month" (Data.gov user guide, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, from_markdown, number, text};

/// What harvested records say when the publisher gave nothing.
const NO_DESCRIPTION: &str = "No description found";
const UNKNOWN_LICENSE: &str = "/unknown-license";

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
    .describe(
        text(row, "/description")
            .filter(|d| d != NO_DESCRIPTION)
            .map(|d| from_markdown(&d)),
    );
    let landing = text(row, "/dcat/landingPage");
    dataset.doi = doi_of(row, landing.as_deref());
    dataset.publisher = text(row, "/dcat/publisher/name")
        .or_else(|| text(row, "/organization/name"));
    dataset.license =
        text(row, "/dcat/license").filter(|l| !l.ends_with(UNKNOWN_LICENSE));
    dataset.updated = day(text(row, "/dcat/modified"));
    dataset.popularity = number(row, "/popularity");
    dataset.aliases.extend(landing);
    dataset.valid()
}

/// Publishers put a DOI in `DOI`, in the identifier, or only as a doi.org
/// landing page.
fn doi_of(row: &Value, landing: Option<&str>) -> Option<String> {
    let resolver_link =
        landing.filter(|page| page.contains("doi.org/")).map(str::to_owned);
    [text(row, "/dcat/DOI"), text(row, "/dcat/identifier"), resolver_link]
        .into_iter()
        .flatten()
        .find_map(|raw| doi(&raw))
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
                popularity: Some(13),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_doi_is_read_from_wherever_the_publisher_put_it() {
        let hits =
            parse(&fixture::json("datagov.identifiers.json"), 10).unwrap();
        let dois: Vec<_> = hits.iter().map(|h| h.doi.as_deref()).collect();
        assert_eq!(
            dois,
            [
                Some("10.25984/1970814"),
                Some("10.3334/ornldaac/1015"),
                Some("10.7289/v5ms3qr9"),
                None,
            ]
        );
        assert_eq!(hits[2].aliases, ["https://doi.org/10.7289/V5MS3QR9"]);
    }

    #[test]
    fn views_become_popularity_and_placeholders_are_dropped() {
        let hits =
            parse(&fixture::json("datagov.identifiers.json"), 10).unwrap();
        let popularity: Vec<_> = hits.iter().map(|h| h.popularity).collect();
        assert_eq!(popularity, [Some(13), Some(1), Some(1), Some(4)]);
        assert_eq!(
            hits[0].license.as_deref(),
            Some("https://creativecommons.org/licenses/by/4.0/")
        );
        assert_eq!(
            hits[3].license, None,
            "the catalog's unknown-license link"
        );
        assert_eq!(hits[3].description, None, "No description found");
    }
}
