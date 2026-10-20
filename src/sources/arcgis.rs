//! ArcGIS Hub's global search (OGC API Records) over public ArcGIS Online
//! items, limited to the `dataset` collection so maps and apps stay out (the
//! other collections are `appAndMap`, `document`, `site` and `all`; Hub Search
//! API, October 2026). `limit` is at most 100 per request, the same as
//! `--per-source`, so one request is enough (Hub Search API definition,
//! October 2026). `q` requires every word, so "air OR quality" matches fewer
//! items than "air quality", and a colon in it matches nothing (probed October
//! 2026). Results come in relevance order, `numberMatched` stops counting at
//! 10,000, and a request that reaches past the 10,000th match answers HTTP
//! 500 (probed October 2026). `license` is an SPDX id that Hub derives from
//! the free-text `licenseInfo`, with `none` for an empty one and `custom` for
//! text it could not map (Hub queryables and aggregations, October 2026).
//! Responses carry an `X-RateLimit-Limit-PORTAL_SEARCH_THROTTLER` header of 10
//! with a reset of 1; the docs state no limit (October 2026). `size` is bytes
//! and `modified` is epoch milliseconds (ArcGIS REST item reference, October
//! 2026). The terms say nothing on caching search results (Esri Master
//! Agreement, August 2025).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, clean, date_from_epoch, items, number, text};

/// A `custom` license is shown as its publisher wrote it only when it reads
/// like a licence name; longer text is terms of use.
const LONGEST_LICENSE: usize = 100;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://hub.arcgis.com/api/search/v1/collections/dataset/items")
        .query("q", query)
        .query("limit", limit)
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("features").is_none() {
        return Err(SourceError::shape("no features array"));
    }
    Ok(items(body, "/features")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(feature: &Value) -> Option<Dataset> {
    let props = feature.get("properties")?;
    let id = text(props, "/id")?;
    let mut dataset = Dataset::new(
        &text(props, "/title")?,
        &format!("https://hub.arcgis.com/datasets/{id}"),
    )
    .describe(
        written(props, "/description").or_else(|| written(props, "/snippet")),
    );
    dataset.publisher =
        text(props, "/source").or_else(|| text(props, "/owner"));
    dataset.license = license(props);
    dataset.updated = number(props, "/modified").and_then(date_from_epoch);
    dataset.size_bytes =
        number(props, "/size").filter(|_| stores_its_data(props));
    dataset.popularity = number(props, "/numViews");
    dataset.valid()
}

/// Item text its owner set. Hub sends the string `null` for some items whose
/// owner never wrote one (probed October 2026).
fn written(props: &Value, pointer: &str) -> Option<String> {
    text(props, pointer).filter(|t| t != "null")
}

fn license(props: &Value) -> Option<String> {
    match text(props, "/license")?.as_str() {
        "none" => None,
        "custom" => written(props, "/licenseInfo")
            .map(|info| clean(&info))
            .filter(|info| {
                info.chars().count() <= LONGEST_LICENSE
                    && info.chars().any(char::is_alphabetic)
            }),
        spdx => Some(spdx.to_owned()),
    }
}

/// A registered service's item only points at a server elsewhere, so its
/// `size` is the item's own hundred-odd bytes; hosted services (typeKeyword
/// `Hosted Service`) and file items measure the data (probed October 2026).
fn stores_its_data(props: &Value) -> bool {
    let has = |keyword: &str| {
        items(props, "/typeKeywords")
            .iter()
            .any(|k| k.as_str() == Some(keyword))
    };
    !has("Service") || has("Hosted Service")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("arcgis.json"), 10).unwrap();
        assert_eq!(hits.len(), 7);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Air Quality Monitoring".into(),
                url: "https://hub.arcgis.com/datasets/\
                      1d11bbdb8bca44a2ad62efa6148f99b9"
                    .into(),
                description: None,
                publisher: Some("Broward County GIS".into()),
                doi: None,
                license: None,
                updated: Some("2026-08-04".into()),
                size_bytes: Some(1658),
                popularity: Some(30),
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[6],
            Dataset {
                title: "Air_Monitoring_Sites".into(),
                url: "https://hub.arcgis.com/datasets/\
                      4dc97eb2f7f544e9a8340ed840f02873"
                    .into(),
                description: Some(
                    "Air Monitoring Sites. Obtained from Oklahoma \
                     Department of Environmental Quality Fall 2015. The \
                     Oklahoma Department of Transportation is not liable \
                     for any inaccuracies in this data."
                        .into()
                ),
                publisher: Some(
                    "Oklahoma Department of Transportation".into()
                ),
                doi: None,
                license: Some("Authorized use only.".into()),
                updated: Some("2016-03-24".into()),
                size_bytes: Some(49_152),
                popularity: Some(192),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn html_descriptions_and_custom_licenses_become_plain_text() {
        let hits = parse(&fixture::json("arcgis.json"), 10).unwrap();
        assert_eq!(
            hits[2].description.as_deref(),
            Some(
                "US EPA feature layer updated to reflect the latest hour of \
                 air quality data (Ozone, etc) received from monitoring \
                 sites that report to AirNow."
            )
        );
        assert_eq!(
            hits[3].description.as_deref(),
            Some(
                "For further information about air quality monitoring - see \
                 the City of York Council website"
            )
        );
        assert_eq!(hits[2].license.as_deref(), Some("ESRI"));
        assert_eq!(hits[3].license, None, "terms of use, not a licence name");
        assert_eq!(
            hits[4].license.as_deref(),
            Some("Licensed under Open Government Licence")
        );
    }

    #[test]
    fn only_hosted_services_and_files_report_a_size() {
        let hits = parse(&fixture::json("arcgis.json"), 10).unwrap();
        let sizes: Vec<_> = hits.iter().map(|h| h.size_bytes).collect();
        assert_eq!(
            sizes,
            [Some(1658), Some(32_768), None, None, None, None, Some(49_152)]
        );
    }

    #[test]
    fn the_string_null_is_no_description() {
        let hits = parse(&fixture::json("arcgis.null.json"), 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "air quality");
        assert_eq!(hits[0].description, None);
    }
}
