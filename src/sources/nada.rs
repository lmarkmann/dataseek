//! NADA, the microdata catalog software of the World Bank, IHSN, FAO, UNHCR
//! and many national statistics offices. Same catalog search everywhere; a
//! registry row per installation picks the base URL.
//!
//! `ps` defaults to 15 and none of the four installations caps it: each
//! answered 1,000 rows, or its whole catalog, in one page, so a search is one
//! request. Rows come in relevance order (NADA, October 2026).
//!
//! The list rows hold no abstract at the World Bank, FAO and UNHCR and one cut
//! at 500 characters at IHSN, so most records have no description. The country
//! and years are not one: `dedup::weigh` drops a hit whose description holds
//! no query word, and NADA matches words in variable labels the list never
//! shows, so a coverage line in its place left 1 of FAO's 100 rows for "water"
//! (NADA, October 2026). FAO and UNHCR run an older NADA that sends numbers as
//! strings and no `status` (NADA, October 2026).

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
        .get(&format!("{base}/api/catalog/search"))
        .query("sk", query)
        .query("ps", limit)
        .json()?;
    parse(base, &body, limit)
}

pub(super) fn parse(
    base: &str,
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/result/rows").is_none() {
        return Err(SourceError::shape("no result.rows"));
    }
    Ok(items(body, "/result/rows")
        .iter()
        .filter_map(|row| record(base, row))
        .take(limit)
        .collect())
}

fn record(base: &str, row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let url =
        text(row, "/url").unwrap_or_else(|| format!("{base}/catalog/{id}"));
    let abstract_text = text(row, "/abstract").filter(|a| a != "null");
    let mut dataset = Dataset::new(&text(row, "/title")?, &url)
        .describe(abstract_text)
        .doi_from(text(row, "/doi"));
    dataset.publisher = text(row, "/authoring_entity");
    dataset.updated = day(text(row, "/changed"));
    dataset.popularity = number(row, "/total_downloads");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(
            "https://microdata.worldbank.org/index.php",
            &fixture::json("nada.json"),
            10,
        )
        .unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Impact Evaluation of Low-Cost In-Line Chlorination \
                        Systems in Urban Dhaka on Water Quality and Child \
                        Health 2015"
                    .into(),
                url: "https://microdata.worldbank.org/catalog/5730".into(),
                description: None,
                publisher: Some(
                    "Stephen P. Luby, Amy Pickering, Sonia Sultana".into()
                ),
                doi: None,
                license: None,
                updated: Some("2023-02-21".into()),
                size_bytes: None,
                popularity: Some(675),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_doi_link_is_read() {
        let hits = parse(
            "https://microdata.worldbank.org/index.php",
            &fixture::json("nada.json"),
            10,
        )
        .unwrap();
        assert_eq!(hits[1].doi.as_deref(), Some("10.48529/6znd-3a32"));
    }

    #[test]
    fn an_abstract_is_the_description_when_the_catalog_sends_one() {
        let hits = parse(
            "https://catalog.ihsn.org/index.php",
            &fixture::json("nada.ihsn.json"),
            10,
        )
        .unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[1],
            Dataset {
                title: "Water Sector 2013".into(),
                url: "https://catalog.ihsn.org/catalog/6241".into(),
                description: Some(
                    "Social Impact (SI) is conducting an impact evaluation \
                     of the MCC Tanzania Water Sector Project."
                        .into()
                ),
                publisher: Some("Social Impact, Inc.".into()),
                doi: None,
                license: None,
                updated: Some("2019-03-29".into()),
                size_bytes: None,
                popularity: Some(72),
                aliases: vec![],
            }
        );
        assert_eq!(hits[0].description, None);
    }

    #[test]
    fn an_older_installation_sends_numbers_as_strings() {
        let hits = parse(
            "https://microdata.fao.org/index.php",
            &fixture::json("nada.fao.json"),
            10,
        )
        .unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Data in Emergencies (DIEM) Monitoring System - \
                        Household Survey - Round 8, Afghanistan, 2024"
                    .into(),
                url: "https://microdata.fao.org/index.php/catalog/2895".into(),
                description: None,
                publisher: Some(
                    "Food and Agriculture Organization of the United \
                     Nations, Data in Emergencies Hub, Office of Emergencies \
                     and Resilience"
                        .into()
                ),
                doi: None,
                license: None,
                updated: Some("2025-12-12".into()),
                size_bytes: None,
                popularity: Some(2),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_row_without_a_url_links_to_its_catalog_page() {
        let mut body = fixture::json("nada.fao.json");
        if let Some(row) = body.pointer_mut("/result/rows/0") {
            row.as_object_mut().unwrap().remove("url");
        }
        let hits =
            parse("https://microdata.fao.org/index.php", &body, 10).unwrap();
        assert_eq!(
            hits[0].url,
            "https://microdata.fao.org/index.php/catalog/2895"
        );
    }
}
