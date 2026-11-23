//! The Dataverse Search API, identical on every installation; a registry row
//! per installation picks the base URL. Results come in relevance order
//! (`sort=score` is the default) and a page holds at most 1000 (a larger
//! `per_page` answers HTTP 400). The query matches any of its words in any
//! indexed field, the parent collection's name included, and tolerates
//! unbalanced quotes, brackets and operators. A dataset item carries no
//! license, size or download count, only `fileCount`; those live in the
//! per-dataset native API, one request each. The API states no rate limit
//! (Dataverse guides 6.12.1 and Harvard Dataverse 6.10.1, October 2026).
//! Harvard Dataverse includes harvested records from other archives (14348
//! of 18270 hits for "air quality"), whose link is their own DOI or handle
//! (Harvard Dataverse, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, text};

pub fn search(
    ctx: &Ctx<'_>,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{base}/api/search"))
        .query("q", query)
        .query("type", "dataset")
        .query("per_page", limit.clamp(1, 1000))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/data/items", limit, record)
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset = Dataset::new(&text(row, "/name")?, &text(row, "/url")?)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/global_id"));
    dataset.publisher = text(row, "/publisher");
    dataset.updated = day(first_text(row, &["/updatedAt", "/published_at"]));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("dataverse.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Climatic characterization of the sites in Argentina \
                        (C\u{f3}rdoba y Santiago del Estero)"
                    .into(),
                url: "https://doi.org/10.7910/DVN/DUWBBU".into(),
                description: Some(
                    "Dateset contains past and future exploration on climate \
                     in Chancan\u{ed}, C\u{f3}rdoba and Copo, Santiago del \
                     Estero, Argentina. The described variables were \
                     proposed by the climatologist, biologist and agronomic \
                     engineers and up -dated with the information obtained \
                     after the stakeholders workshop."
                        .into()
                ),
                publisher: Some("Climate variability in the Americas".into()),
                doi: Some("10.7910/dvn/duwbbu".into()),
                license: None,
                updated: Some("2025-06-16".into()),
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    fn first(fixture_name: &str) -> Dataset {
        let hits = parse(&fixture::json(fixture_name), 10).unwrap();
        assert_eq!(hits.len(), 4, "{fixture_name}");
        hits.into_iter().next().unwrap()
    }

    #[test]
    fn darus_names_the_collection_as_publisher() {
        assert_eq!(
            first("dataverse.darus.json"),
            Dataset {
                title: "Replication Data for: Biocatalytic stereocontrolled \
                        head-to-tail cyclizations of unbiased terpenes as a \
                        tool in chemoenzymatic synthesis"
                    .into(),
                url: "https://doi.org/10.18419/DARUS-4135".into(),
                description: Some(
                    "This data set contains all relevant simulation input \
                     files (topologies, coordinates, simulation parameters), \
                     generated simulation output (final configurations, time \
                     series of collective variables) together with scripts \
                     used for set-up and analysis of the umbrella sampling \
                     and double decoupling simulations."
                        .into()
                ),
                publisher: Some("Molecular Simulation".into()),
                doi: Some("10.18419/darus-4135".into()),
                updated: Some("2024-06-11".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn juelich_publisher_loses_its_leading_space() {
        assert_eq!(
            first("dataverse.juelich.json"),
            Dataset {
                title: "SpEx - Speech and executive function".into(),
                url: "https://doi.org/10.26165/JUELICH-DATA/CHWZDZ".into(),
                description: Some(
                    "The goal of this study was to generate a dataset that \
                     capitalises on both the rich variety of attributes that \
                     can be found in speech as well as the modern methods \
                     that allow the extraction of such attributes."
                        .into()
                ),
                publisher: Some(
                    "Institute of Neuroscience and Medicine \u{2013} Brain \
                     and Behaviour (INM-7)"
                        .into()
                ),
                doi: Some("10.26165/juelich-data/chwzdz".into()),
                updated: Some("2024-04-16".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn aussda_dates_a_record_by_its_last_update() {
        assert_eq!(
            first("dataverse.aussda.json"),
            Dataset {
                title: "Political Election Issues, Government Participation, \
                        Election Standard 1999"
                    .into(),
                url: "https://doi.org/10.11587/FEWVOJ".into(),
                description: Some(
                    "Topic: Political election issues, government \
                     participation, election standard"
                        .into()
                ),
                publisher: Some("GfK Austria".into()),
                doi: Some("10.11587/fewvoj".into()),
                updated: Some("2026-01-29".into()),
                ..Dataset::default()
            },
            "updatedAt, not the 2023 publication"
        );
    }

    #[test]
    fn cora_maps_its_first_record() {
        assert_eq!(
            first("dataverse.cora.json"),
            Dataset {
                title: "Semi-synthetic dataset: motor vehicle crashes and \
                        temperature in Catalonia"
                    .into(),
                url: "https://doi.org/10.34810/DATA235".into(),
                description: Some(
                    "Semi-synthetic dataset that was created to approximate \
                     the relationship between the daily number of motor \
                     vehicle crashes and ambient temperature in 14 climatic \
                     regions of Catalonia (warm seasons of years 2000-2011)"
                        .into()
                ),
                publisher: Some(
                    "Climate, Air Pollution, Nature and Urban Health".into()
                ),
                doi: Some("10.34810/data235".into()),
                updated: Some("2025-03-24".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn qdr_maps_its_first_record() {
        assert_eq!(
            first("dataverse.qdr.json"),
            Dataset {
                title: "Demand for Modern Contraception in Sub-Saharan \
                        Africa: New Methods, New Evidence"
                    .into(),
                url: "https://doi.org/10.5064/F600CMLO".into(),
                description: Some(
                    "Project Overview West and Central Africa have the \
                     highest fertility rates in the world. These high \
                     birthrates are largely a function of desired fertility \
                     and continued allegiance to large families."
                        .into()
                ),
                publisher: Some("QDR Main Collection".into()),
                doi: Some("10.5064/f600cmlo".into()),
                updated: Some("2026-04-09".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn a_query_matching_nothing_is_an_empty_answer() {
        let body = serde_json::json!({
            "status": "OK",
            "data": {
                "q": "zzqxjvkw qqxzjv",
                "total_count": 0,
                "start": 0,
                "spelling_alternatives": {},
                "items": [],
                "count_in_response": 0
            }
        });
        assert_eq!(parse(&body, 10).unwrap(), vec![]);
    }

    #[test]
    fn an_error_body_is_a_changed_shape() {
        let body = serde_json::json!({
            "status": "ERROR",
            "message": "1001 results per page requested but max limit is 1000."
        });
        assert!(matches!(parse(&body, 10), Err(SourceError::Shape(_))));
    }
}
