//! The OpenDataSoft federated hub (data.opendatasoft.com), which indexes the
//! public datasets of OpenDataSoft portals: French cities and regions,
//! utilities, Swiss and UK councils. Explore API v2.1, ODSQL `search()`.
//!
//! - One page covers any per-source count: `limit` is at most 100 and
//!   `offset + limit` must stay under 10,000, so nothing here pages
//!   (OpenDataSoft Explore API reference, October 2026).
//! - Anonymous calls share a quota of 10,000 a day that resets at midnight
//!   UTC; the 429 body reads "Too many requests on the domain" (Explore API
//!   reference and `X-RateLimit-Limit` header, October 2026).
//! - The hub's own dataset pages redirect to hub.huwise.com since the portal
//!   closed to the public, while the API stays up (OpenDataSoft community
//!   announcement, July 2025; redirect checked October 2026). A record links
//!   to the portal that publishes it, from `source_domain_address`.
//! - A string literal in `search()` ends at an unescaped quote, and a lone
//!   backslash or a line break inside it answers HTTP 400 (probe, October
//!   2026).
//! - `references` is the "link of the source of the dataset" (Huwise user
//!   guide, October 2026); PNDB fills it with the dataset's own DOI link.

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
        .get("https://data.opendatasoft.com/api/explore/v2.1/catalog/datasets")
        .query("where", format!("search(\"{}\")", literal(query)))
        .query("limit", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

fn literal(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
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
    let meta = row.pointer("/metas/default")?;
    let portal = text(meta, "/source_domain_address")?;
    let id = text(meta, "/source_dataset")?;
    let mut dataset = Dataset::new(
        &text(meta, "/title")?,
        &format!("https://{portal}/explore/dataset/{id}/"),
    )
    .describe(
        text(meta, "/description").map(|d| without_source_link(&d).to_owned()),
    )
    .doi_from(
        text(meta, "/references")
            .filter(|link| link.starts_with("https://doi.org/")),
    );
    dataset.publisher = text(meta, "/publisher")
        .or_else(|| text(meta, "/source_domain_title"));
    dataset.license = text(meta, "/license");
    dataset.updated = day(text(meta, "/modified"));
    dataset.valid()
}

/// PNDB's harvester opens every description with a link to the record it
/// copied, labelled "Lien vers la fiche source", then a line break.
fn without_source_link(html: &str) -> &str {
    html.strip_prefix("<a ")
        .and_then(|rest| rest.split_once("</a>"))
        .and_then(|(_, after)| after.trim_start().strip_prefix("<br>"))
        .unwrap_or(html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("opendatasoft.json"), 10).unwrap();
        assert_eq!(hits.len(), 6);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Nitrate concentration parameters in the water column \
                        | Concentration of nitrate {NO3} per unit volume of \
                        the water body [unknown phase] | EMODNet Chemistry 2 \
                        | Black Sea DIVA 4D analysis of Water_body_nitrate - \
                        Summer"
                    .into(),
                url: "https://pndb.opendatasoft.com/explore/dataset/\
                      nitrate-concentration-parameters-in-the-water-column-\
                      concentration-of-nitrate-no3-per-unit-volume-of-the-\
                      water-body-unknown-phase-emodnet-chemistry-2-black-sea-\
                      diva-4d-analysis-of-water_body_nitrate-summer/"
                    .into(),
                description: None,
                publisher: Some("PNDB".into()),
                doi: None,
                license: None,
                updated: Some("2018-03-26".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_record_links_to_the_portal_that_publishes_it() {
        let hits = parse(&fixture::json("opendatasoft.json"), 10).unwrap();
        let bristol = &hits[4];
        assert_eq!(bristol.title, "Surface Water Sampling Points");
        assert_eq!(
            bristol.url,
            "https://opendata.bristol.gov.uk/explore/dataset/\
             surface-water-sampling-points/"
        );
        assert_eq!(
            bristol.publisher.as_deref(),
            Some("Bristol City Council - Sustainability Team")
        );
        assert_eq!(
            bristol.license.as_deref(),
            Some("Open Government Licence v3.0")
        );
        assert_eq!(bristol.updated.as_deref(), Some("2018-10-10"));
    }

    #[test]
    fn the_source_link_label_is_not_the_description() {
        let hits = parse(&fixture::json("opendatasoft.json"), 10).unwrap();
        for hit in &hits[..2] {
            assert_eq!(hit.description, None, "{}", hit.title);
        }
        let described = hits[2].description.as_deref().unwrap();
        assert!(
            described.starts_with(
                "The observations of campe glider on MOOSE T00_35 deployment"
            ),
            "{described}"
        );
    }

    #[test]
    fn a_doi_link_in_references_is_the_doi() {
        let hits = parse(&fixture::json("opendatasoft.json"), 10).unwrap();
        assert_eq!(hits[5].doi.as_deref(), Some("10.57745/hou0ef"));
        assert_eq!(
            hits[5].publisher.as_deref(),
            Some("Gindrat-Keller, Cl\u{e9}ment")
        );
        assert_eq!(hits[3].doi, None, "a catalogue URL is not a DOI");
    }

    #[test]
    fn the_query_is_one_odsql_string_literal() {
        assert_eq!(literal("C:\\temp"), "C:\\\\temp");
        assert_eq!(literal("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(literal("water\nlevel  rise"), "water level rise");
    }
}
