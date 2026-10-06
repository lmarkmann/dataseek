//! CKAN's `package_search`, the action API shared by hundreds of open-data
//! portals. Each [`Portal`] pairs the API root with the public page prefix,
//! because several portals serve the API from a different host than their
//! website (data.gov.uk runs it on ckan.publishing.service.gov.uk).
//!
//! `rows` takes up to 1000 unless a site lowers `ckan.search.rows_max`, and
//! all six portals answered `rows=100` in full, so one page serves the
//! largest per-source count (CKAN API guide, probes, October 2026). Canada
//! also indexes publications and web pages as `info` records (315 of 2,858
//! matches for "climate"); `dataset_type:dataset` drops them and changes no
//! count on the other five (probes, October 2026). Descriptions are Markdown
//! with HTML mixed in: CKAN renders `notes` with `render_markdown`, and 95 of
//! 100 HDX notes carry links (CKAN templates, probes, October 2026). A
//! record's `metadata_modified` is often the day a portal harvested it, so
//! the dataset's own date wins where a portal sends one: `last_modified` on
//! HDX, `modified` on GovData, `dcat_modified` on data.gov.uk and
//! `remote_last_updated` on data.gov.au (probes, October 2026). HDX allows
//! 60 requests a minute (HDX API docs, October 2026); data.gov.uk states no
//! limit (data.gov.uk API guide, October 2026).
//!
//! On GovData `organization` names the portal that harvested a record
//! (GDI-DE, Mobilithek), while the `publisher_name` extra names who published
//! it (probes, October 2026). A license longer than 100 characters is a whole
//! pasted statement, as in 6 of 100 data.gov.au titles; CKAN's own
//! `notspecified` id, 32 of those 100, is no license at all (probes, CKAN
//! `license.py`, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, from_markdown, items, number, text};

const LONGEST_LICENSE: usize = 100;

pub struct Portal {
    pub api: &'static str,
    pub page: &'static str,
    /// Organization titles read "English | French".
    pub bilingual_orgs: bool,
}

pub static DATA_GOV_UK: Portal = Portal {
    api: "https://ckan.publishing.service.gov.uk/api/action",
    page: "https://www.data.gov.uk/dataset/",
    bilingual_orgs: false,
};
pub static OPEN_CANADA: Portal = Portal {
    api: "https://open.canada.ca/data/api/action",
    page: "https://open.canada.ca/data/en/dataset/",
    bilingual_orgs: true,
};
pub static DATA_GOV_AU: Portal = Portal {
    api: "https://data.gov.au/data/api/3/action",
    page: "https://data.gov.au/data/dataset/",
    bilingual_orgs: false,
};
pub static GOVDATA: Portal = Portal {
    api: "https://ckan.govdata.de/api/3/action",
    page: "https://www.govdata.de/suche/daten/",
    bilingual_orgs: false,
};
pub static HDX: Portal = Portal {
    api: "https://data.humdata.org/api/3/action",
    page: "https://data.humdata.org/dataset/",
    bilingual_orgs: false,
};
pub static B2FIND: Portal = Portal {
    api: "https://b2find.eudat.eu/api/3/action",
    page: "https://b2find.eudat.eu/dataset/",
    bilingual_orgs: false,
};

pub fn search(
    ctx: &Ctx<'_>,
    portal: &Portal,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{}/package_search", portal.api))
        .query("q", query)
        .query("fq", "dataset_type:dataset")
        .query("rows", limit)
        .json()?;
    parse(portal, &body, limit)
}

pub(super) fn parse(
    portal: &Portal,
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(SourceError::shape("CKAN did not report success"));
    }
    Ok(items(body, "/result/results")
        .iter()
        .filter_map(|row| record(portal, row))
        .take(limit)
        .collect())
}

fn record(portal: &Portal, row: &Value) -> Option<Dataset> {
    let name = text(row, "/name")?;
    let title = text(row, "/title").unwrap_or_else(|| name.clone());
    let extra = |key: &str| {
        items(row, "/extras").iter().find_map(|e| {
            (e.get("key").and_then(Value::as_str) == Some(key))
                .then(|| text(e, "/value"))
                .flatten()
        })
    };
    let doi = extra("DOI")
        .or_else(|| text(row, "/digital_object_identifier"))
        .or_else(|| {
            extra("identifier")
                .filter(|id| id.starts_with("10.") || id.contains("doi.org/"))
        });
    let mut dataset = Dataset::new(&title, &format!("{}{name}", portal.page))
        .describe(text(row, "/notes").map(|notes| from_markdown(&notes)))
        .doi_from(doi);
    dataset.publisher = extra("publisher_name")
        .or_else(|| {
            text(row, "/organization/title").map(|title| {
                match title.split_once(" | ") {
                    Some((english, _)) if portal.bilingual_orgs => {
                        english.to_owned()
                    }
                    _ => title,
                }
            })
        })
        .or_else(|| extra("Publisher"))
        .or_else(|| text(row, "/author"));
    dataset.license = text(row, "/license_title")
        .or_else(|| {
            items(row, "/resources").iter().find_map(|r| text(r, "/license"))
        })
        .filter(|license| license.chars().count() <= LONGEST_LICENSE)
        .filter(|_| {
            text(row, "/license_id").as_deref() != Some("notspecified")
        });
    dataset.updated = day(text(row, "/last_modified")
        .or_else(|| extra("modified"))
        .or_else(|| extra("dcat_modified"))
        .or_else(|| text(row, "/remote_last_updated"))
        .or_else(|| text(row, "/metadata_modified")));
    dataset.popularity = number(row, "/total_res_downloads");
    if let Some(source_page) = text(row, "/url")
        && source_page.starts_with("http")
    {
        dataset.aliases.push(source_page);
    }
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits =
            parse(&DATA_GOV_UK, &fixture::json("ckan.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Climate resilience documents".into(),
                url: "https://www.data.gov.uk/dataset/\
                      climate-resilience-documents"
                    .into(),
                description: Some(
                    "This dataset includes links to policies, strategies and \
                     documents relevant to climate resilience on a wide \
                     range of geographic scales. The project was undertaken \
                     with the guidance of Leeds City Council and Leeds \
                     Climate Commission and contains a large amount of Leeds \
                     specific policies and data."
                        .into()
                ),
                publisher: Some("Data Mill North".into()),
                doi: None,
                license: None,
                updated: Some("2018-10-17".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![
                    "https://datamillnorth.org/dataset/\
                     climate-resilience-documents-vdwno"
                        .into()
                ],
            }
        );
        assert_eq!(
            hits[3].license.as_deref(),
            Some("UK Open Government Licence (OGL)")
        );
        assert_eq!(
            hits[2].description.as_deref(),
            Some(
                "The ' Climate Just' Map Tool shows the geography of \
                 England\u{2019}s vulnerability to climate change at a \
                 neighbourhood scale."
            )
        );
    }

    #[test]
    fn hdx_links_become_text_and_downloads_count() {
        let hits = parse(&HDX, &fixture::json("ckan.hdx.json"), 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Climate Change Opinion Survey".into(),
                url: "https://data.humdata.org/dataset/\
                      climate-change-opinion-survey"
                    .into(),
                description: Some(
                    "In partnership with Yale, Meta launched a climate \
                     change opinion survey that explores public climate \
                     change knowledge, attitudes, policy preferences, and \
                     behaviors. 2023 aggregated survey responses now \
                     available. The 2022 survey includes respondents from \
                     nearly 200 countries and territories."
                        .into()
                ),
                publisher: Some("AI for Good at Meta".into()),
                doi: None,
                license: Some("Public Domain / No restrictions (CC0)".into()),
                updated: Some("2025-03-04".into()),
                size_bytes: None,
                popularity: Some(29178),
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[1].description.as_deref(),
            Some(
                "Contains data from the World Bank's data portal. There is \
                 also a consolidated country dataset on HDX. Climate change \
                 is expected to hit developing countries the hardest."
            )
        );
        assert_eq!(hits[1].updated.as_deref(), Some("2026-07-16"));
        assert_eq!(hits[1].popularity, Some(720));
    }

    #[test]
    fn canada_publishes_in_english_and_keeps_only_real_links() {
        let hits = parse(&OPEN_CANADA, &fixture::json("ckan.canada.json"), 10)
            .unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Climatic Regions".into(),
                url: "https://open.canada.ca/data/en/dataset/\
                      09ffaeb5-ec8f-5bb5-bdcb-3436ccf26f58"
                    .into(),
                description: Some(
                    "Contained within 3rd Edition (1957) of the Atlas of \
                     Canada is a map that shows the division of Canada into \
                     climatic regions according to the classification of the \
                     climates of the world developed by W. Koppen. Koppen \
                     first divided the world into five major divisions to \
                     which he assigned the letters A, B, C, D, and E."
                        .into()
                ),
                publisher: Some("Natural Resources Canada".into()),
                doi: None,
                license: Some("Open Government Licence - Canada".into()),
                updated: Some("2022-03-14".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            },
            "`url` there is a Python dict of two API links, not a page"
        );
        assert_eq!(
            hits[1].doi.as_deref(),
            Some("10.23687/c1891184-b6dc-4dc7-95b3-ecf108b02a8d")
        );
        assert_eq!(
            hits[2].doi.as_deref(),
            Some("10.23687/72e3304f-4103-4060-9bf0-fdd1325b6851")
        );
    }

    #[test]
    fn govdata_names_the_publisher_and_reads_dcat_dates_and_licenses() {
        let hits =
            parse(&GOVDATA, &fixture::json("ckan.govdata.json"), 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Ecomapper Dataset".into(),
                url: "https://www.govdata.de/suche/daten/ecomapper-dataset"
                    .into(),
                description: Some(
                    "The EcoMapper dataset comprises over 2.9 million \
                     satellite images accompanied by climate metadata. It \
                     includes both RGB imagery and selected multispectral \
                     channels (B6 \u{2013} Red Edge 2, B8 \u{2013} NIR, B11 \
                     \u{2013} SWIR1) sourced from the Copernicus Sentinel \
                     satellite missions."
                        .into()
                ),
                publisher: Some(
                    "Universit\u{e4}tsbibliothek der Technischen Universit\
                     \u{e4}t M\u{fc}nchen"
                        .into()
                ),
                doi: None,
                license: Some(
                    "http://dcat-ap.de/def/licenses/other-closed".into()
                ),
                updated: Some("2026-09-09".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            },
            "no `modified` extra, so the portal's own record date stands"
        );
        assert_eq!(hits[1].doi.as_deref(), Some("10.14459/2025mp1767651"));
        assert_eq!(hits[1].updated.as_deref(), Some("2025-06-13"));
        assert_eq!(
            hits[2].doi.as_deref(),
            Some("10.5282/ubm/data.189"),
            "the `identifier` extra spelled as a resolver link"
        );
        assert_eq!(hits[2].updated.as_deref(), Some("2021-02-08"));
        assert_eq!(
            hits[2].license.as_deref(),
            Some("http://dcat-ap.de/def/licenses/cc-by/4.0")
        );
    }

    #[test]
    fn data_gov_au_headings_dates_and_placeholder_licenses() {
        let hits =
            parse(&DATA_GOV_AU, &fixture::json("ckan.australia.json"), 10)
                .unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Agricultural climate metrics for the National \
                        Climate Risk Assessment"
                    .into(),
                url: "https://data.gov.au/data/dataset/fedora-pid_csiro-64751"
                    .into(),
                description: Some(
                    "Agricultural climate metrics were derived for the \
                     National Climate Risk Assessment."
                        .into()
                ),
                publisher: Some("CSIRO Data Access Portal".into()),
                doi: None,
                license: None,
                updated: Some("2025-11-30".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            },
            "CKAN's `notspecified` license id is no license"
        );
        assert_eq!(hits[1].license.as_deref(), Some("cc-by-4"));
        assert_eq!(
            hits[1].updated.as_deref(),
            Some("2023-02-20"),
            "`remote_last_updated`, not the day the portal harvested it"
        );
        assert_eq!(
            hits[2].description.as_deref(),
            Some(
                "Abstract The dataset was derived by the Bioregional \
                 Assessment Programme from the national SILO data sets of \
                 climate station records. The parent dataset(s) is \
                 identified in the Lineage field in this metadata statement."
            )
        );
        assert_eq!(hits[2].license, None, "a 275-character credit line");
        assert_eq!(hits[2].updated.as_deref(), Some("2022-04-13"));
        assert_eq!(hits[3].updated.as_deref(), Some("2025-07-29"));
    }
}
