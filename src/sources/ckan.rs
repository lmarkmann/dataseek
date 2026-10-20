//! CKAN's `package_search`, the action API shared by hundreds of open-data
//! portals. Each [`Portal`] pairs the API root with the public page prefix,
//! because several portals serve the API from a different host than their
//! website (data.gov.uk runs it on ckan.publishing.service.gov.uk).
//!
//! `rows` takes up to 1000 unless a site lowers `ckan.search.rows_max`, and
//! all six portals answered `rows=100` in full, so one page serves the
//! largest per-source count; HDX clamps `rows=1001` to 1000 without an error
//! (CKAN API guide, HDX `package_search` docs, probes, October 2026). A colon
//! anywhere in `q` switches CKAN from its dismax parser to a fielded Lucene
//! query, so "covid: deaths" matched nothing on four portals, escaped or not,
//! and "air:" is a 409 Search Error; the query goes out with colons as spaces
//! (CKAN `lib/search/query.py`, probes, October 2026). Canada also indexes
//! publications and web pages as `info` records (315 of 2,858 matches for
//! "climate"); `dataset_type:dataset` drops them and changes no count on the
//! other five (probes, October 2026). Descriptions are Markdown with HTML
//! mixed in: CKAN renders `notes` with `render_markdown`, and 95 of 100 HDX
//! notes carry links (CKAN templates, probes, October 2026). A record's
//! `metadata_modified` is often the day a portal harvested it, so the
//! dataset's own date wins where a portal sends one: `last_modified` on HDX,
//! `modified` on GovData, `dcat_modified` on data.gov.uk and
//! `remote_last_updated` on data.gov.au (probes, October 2026). HDX allows
//! 60 requests a minute anonymously and answers more with 429 (HDX metadata
//! endpoint troubleshooting page, October 2026); data.gov.uk states "there
//! are no rate limits" (data.gov.uk API guide, October 2026).
//!
//! On GovData `organization` names the portal that harvested a record
//! (GDI-DE, Mobilithek), while the `publisher_name` extra names who published
//! it (probes, October 2026). A license longer than 100 characters is a whole
//! pasted statement, as in 6 of 100 data.gov.au titles; CKAN's own
//! `notspecified` id, 32 of those 100, is no license at all (probes, CKAN
//! `license.py`, October 2026).
//!
//! A resource's `size` is bytes: HDX's 494,307,199 for a WorldPop GeoTIFF is
//! that file's Content-Length. HDX states a size for every resource of 326 of
//! 329 records across four queries, links to other hosts included; the other
//! portals do so for 3 to 12 of 100 (probes, October 2026). A record's size
//! is the sum only when every resource states one, since a partial sum would
//! understate it. HDX has no DOI field; the DOIs in its notes and
//! `methodology_other` cite papers and upstream products, and WorldPop's
//! per-country records all cite the same global ones (probes, October 2026).
//!
//! B2FIND sends no resources and no `license_title`. Its extras carry the
//! harvested DataCite record: `Publisher` names the publisher where the
//! organization is the harvesting community ("da|ra", "CESSDA"), `Rights`
//! mixes licenses with access rights, and `Size` lists byte counts from
//! Dataverse installations ("6828; 6803"), rounded sizes from B2SHARE
//! ("215.9 kB; 1 file") or counts that are not bytes ("28254 data points")
//! (probes, October 2026).
//!
//! Each resource's `format` is the publisher's free text: "CSV", "XLSX",
//! "SHP", "JPG", "BIN", or empty on GovData; B2FIND sends no resources and
//! so no formats (probes, October 2026). `file_format` turns the spellings
//! into one name per format.
//!
//! `--updated-since` goes out as a `metadata_modified` range in `fq`, which
//! all six portals honor. The date a record shows is never later than its
//! `metadata_modified` (600 records on data.gov.uk, Canada and data.gov.au),
//! so the range drops nothing the filter on the merged list keeps; on
//! GovData, HDX and B2FIND it drops nothing at all, since every record there
//! was re-harvested after 2024 (probes, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, clean, day, from_markdown, items, number, text};

const LONGEST_LICENSE: usize = 100;

/// Words by which a part of B2FIND's `Rights` names a license rather than
/// an access right or the archive that holds the data.
const LICENSE_WORDS: [&str; 8] = [
    "licen",
    "creative commons",
    "cc0",
    "cc-by",
    "cc by",
    "odbl",
    "etalab",
    "public domain",
];

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
        .query("q", query.replace(':', " "))
        .query("fq", filter_query(ctx.since))
        .query("rows", limit)
        .json()?;
    parse(portal, &body, limit)
}

fn filter_query(since: Option<&str>) -> String {
    let datasets = "dataset_type:dataset";
    since.map_or_else(
        || datasets.to_owned(),
        |day| {
            format!("{datasets} AND metadata_modified:[{day}T00:00:00Z TO *]")
        },
    )
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
        .or_else(|| extra("Publisher"))
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
        .or_else(|| text(row, "/author"));
    dataset.license = text(row, "/license_title")
        .or_else(|| {
            items(row, "/resources").iter().find_map(|r| text(r, "/license"))
        })
        .or_else(|| extra("Rights").and_then(|rights| named_license(&rights)))
        .filter(|license| license.chars().count() <= LONGEST_LICENSE)
        .filter(|_| {
            text(row, "/license_id").as_deref() != Some("notspecified")
        });
    dataset.updated = [
        text(row, "/last_modified"),
        extra("modified"),
        extra("dcat_modified"),
        text(row, "/remote_last_updated"),
        text(row, "/metadata_modified"),
    ]
    .into_iter()
    .find_map(day);
    dataset.size_bytes = resource_bytes(row)
        .or_else(|| extra("Size").and_then(|sizes| listed_bytes(&sizes)));
    dataset.popularity = number(row, "/total_res_downloads");
    dataset.formats = items(row, "/resources")
        .iter()
        .filter_map(|resource| text(resource, "/format"))
        .collect();
    if let Some(source_page) = text(row, "/url")
        && source_page.starts_with("http")
    {
        dataset.aliases.push(source_page);
    }
    dataset.valid()
}

fn resource_bytes(row: &Value) -> Option<u64> {
    let resources = items(row, "/resources");
    if resources.is_empty() {
        return None;
    }
    resources.iter().try_fold(0, |total: u64, resource| {
        total.checked_add(number(resource, "/size").filter(|&size| size > 0)?)
    })
}

fn named_license(rights: &str) -> Option<String> {
    rights
        .split(';')
        .find(|part| {
            let lower = part.to_lowercase();
            LICENSE_WORDS.iter().any(|word| lower.contains(word))
        })
        .map(clean)
}

/// The parts of B2FIND's `Size` that are byte sizes, summed; `None` when no
/// part is.
fn listed_bytes(sizes: &str) -> Option<u64> {
    let bytes: Vec<u64> = sizes.split(';').filter_map(bytes_in).collect();
    if bytes.is_empty() {
        return None;
    }
    bytes.into_iter().try_fold(0, u64::checked_add)
}

/// "6828", "23063374 Bytes", "215.9 kB" or "40.9 MBytes" as bytes, the units
/// read as powers of 1000.
fn bytes_in(size: &str) -> Option<u64> {
    let size = size.trim();
    let (amount, unit) = size.split_once(' ').unwrap_or((size, "bytes"));
    let scale: u64 = match unit.to_ascii_lowercase().as_str() {
        "b" | "bytes" => 1,
        "kb" | "kbytes" => 1_000,
        "mb" | "mbytes" => 1_000_000,
        "gb" | "gbytes" => 1_000_000_000,
        "tb" | "tbytes" => 1_000_000_000_000,
        _ => return None,
    };
    let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
    let digits: u64 = format!("{whole}{fraction}").parse().ok()?;
    let places = u32::try_from(fraction.len()).ok()?;
    digits.checked_mul(scale)?.checked_div(10_u64.checked_pow(places)?)
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
                size_bytes: Some(18_759 + 22_892 + 24_498 + 24_412),
                popularity: None,
                formats: vec![
                    "CSV".into(),
                    "XLSX".into(),
                    "XLSX".into(),
                    "CSV".into()
                ],
                aliases: vec![
                    "https://datamillnorth.org/dataset/\
                     climate-resilience-documents-vdwno"
                        .into()
                ],
            },
            "the fixture keeps 4 of the record's 6 resources"
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
        assert_eq!(hits[1].size_bytes, None, "its one resource has no size");
        assert_eq!(hits[3].size_bytes, None);
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
                size_bytes: Some(787_522),
                popularity: Some(29178),
                formats: vec!["CSV".into()],
                aliases: vec![],
            },
            "the fixture keeps 1 of the record's 6 resources"
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
        assert_eq!(hits[1].size_bytes, Some(149_602));
    }

    #[test]
    fn hdx_sums_the_sizes_of_every_resource() {
        let hits =
            parse(&HDX, &fixture::json("ckan.hdx-air-quality.json"), 10)
                .unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Ecuador: Monthly Air Quality in Quito".into(),
                url: "https://data.humdata.org/dataset/\
                      air-quality-quito-monthly-2018-2020"
                    .into(),
                description: Some(
                    "Monthly air quality data based on the parameters of CO, \
                     wind direction, humidity, NO2, O3, particulate matter, \
                     SO2."
                        .into()
                ),
                publisher: Some("Hub Latin America".into()),
                doi: None,
                license: Some(
                    "Creative Commons Attribution Share-Alike (CC BY-SA)"
                        .into()
                ),
                updated: Some("2021-08-16".into()),
                size_bytes: Some(27_652),
                popularity: Some(78),
                formats: vec!["SHP".into()],
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[2].size_bytes,
            Some(
                5_432_680
                    + 5_936_743
                    + 35_407_393
                    + 36_746_010
                    + 1_201_928
                    + 1_259_896
                    + 1_230_837
            ),
            "all seven CSV files"
        );
        assert_eq!(hits[2].updated.as_deref(), Some("2026-10-05"));
        assert_eq!(hits[2].popularity, Some(644));
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
                size_bytes: Some(1_134_541),
                popularity: None,
                formats: vec!["JPG".into()],
                aliases: vec![],
            },
            "`url` there is a Python dict of two API links, not a page"
        );
        assert_eq!(
            hits[1].doi.as_deref(),
            Some("10.23687/c1891184-b6dc-4dc7-95b3-ecf108b02a8d")
        );
        assert_eq!(hits[1].size_bytes, Some(273_122));
        assert_eq!(
            hits[2].doi.as_deref(),
            Some("10.23687/72e3304f-4103-4060-9bf0-fdd1325b6851")
        );
        assert_eq!(hits[2].size_bytes, None);
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
                formats: vec![],
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
                size_bytes: Some(178_264),
                popularity: None,
                formats: vec!["BIN".into()],
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
        assert_eq!(hits[2].size_bytes, Some(253_214));
        assert_eq!(hits[3].updated.as_deref(), Some("2025-07-29"));
    }

    #[test]
    fn b2find_reads_the_harvested_datacite_extras() {
        let hits =
            parse(&B2FIND, &fixture::json("ckan.b2find.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Historical Air Quality Data Austria Umweltbundesamt"
                    .into(),
                url: "https://b2find.eudat.eu/dataset/\
                      9721faf6-3c40-55ea-beeb-9dda97b4b6dc"
                    .into(),
                description: Some(
                    "This digital record contains historical air pollution \
                     and air quality data from approximately 20 air \
                     monitoring stations in Vienna, spanning the years from \
                     1980 to 2021. The data was provided by the \
                     Umweltbundesamt and is stored in its original form in \
                     this record."
                        .into()
                ),
                publisher: Some("TU Wien".into()),
                doi: Some("10.48436/mtha8-w2406".into()),
                license: Some(
                    "Creative Commons Attribution 4.0 International".into()
                ),
                updated: Some("2025-06-20".into()),
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            },
            "no `Size` extra and no resources"
        );
        assert_eq!(
            hits[1].publisher.as_deref(),
            Some("DANS Data Station Phys-Tech Sciences")
        );
        assert_eq!(hits[1].license.as_deref(), Some("CC0 1.0"));
        assert_eq!(
            hits[1].description.as_deref(),
            Some(
                "Despite increased awareness of climate change and urban \
                 air pollution, little research has been performed to \
                 examine the influence of meteorology and air quality on \
                 athletic performance of the general public and \
                 recreational exercisers."
            )
        );
        assert_eq!(
            hits[1].size_bytes,
            Some(
                6828 + 6803
                    + 6813
                    + 6799
                    + 6824
                    + 6817
                    + 6833
                    + 6823
                    + 6821
                    + 6829
                    + 6830
                    + 6827
                    + 6810
                    + 5062
                    + 36_694
                    + 1_148_641
                    + 4391
                    + 3774
                    + 29_598
                    + 31_009
                    + 40_959_504
                    + 140_276_743
                    + 8782
                    + 186_574
                    + 170_780
                    + 2155
            ),
            "Dataverse's file sizes, one per file"
        );
        assert_eq!(hits[2].size_bytes, None, "28254 data points");
        assert_eq!(
            hits[2].license.as_deref(),
            Some("Creative Commons Attribution 3.0 Unported")
        );
        assert_eq!(hits[2].doi.as_deref(), Some("10.1594/pangaea.872005"));
        assert_eq!(hits[3].size_bytes, Some(215_900), "215.9 kB; 1 file");
        assert_eq!(
            hits[3].license.as_deref(),
            Some("Creative Commons Attribution (CC-BY)")
        );
        assert_eq!(hits[3].publisher.as_deref(), Some("EUDAT B2SHARE"));
        assert_eq!(hits[3].doi, None, "the DOI in its notes cites a paper");
        assert_eq!(
            hits[3].aliases,
            vec![
                "https://b2share.fz-juelich.de/records/\
                 be5cde68c6ba4ab08b6687b17adbb3b6"
            ]
        );
    }

    #[test]
    fn rights_yield_the_part_that_names_a_license() {
        assert_eq!(
            named_license(
                "etalab 2.0; info:eu-repo/semantics/openAccess; \
                 https://spdx.org/licenses/etalab-2.0.html"
            )
            .as_deref(),
            Some("etalab 2.0")
        );
        assert_eq!(named_license("info:eu-repo/semantics/openAccess"), None);
        assert_eq!(
            named_license(
                "Access to data through SND. Data are freely accessible."
            ),
            None
        );
    }

    #[test]
    fn a_date_asks_for_records_modified_since() {
        assert_eq!(filter_query(None), "dataset_type:dataset");
        assert_eq!(
            filter_query(Some("2024-07-01")),
            "dataset_type:dataset AND \
             metadata_modified:[2024-07-01T00:00:00Z TO *]"
        );
    }

    #[test]
    fn sizes_in_units_become_bytes() {
        assert_eq!(bytes_in("23063374 Bytes"), Some(23_063_374));
        assert_eq!(bytes_in(" 40.9 MBytes"), Some(40_900_000));
        assert_eq!(bytes_in("22.2 GB"), Some(22_200_000_000));
        assert_eq!(bytes_in("6 files"), None);
        assert_eq!(bytes_in("unknown"), None);
        assert_eq!(listed_bytes("437.6 MB; 144 files"), Some(437_600_000));
        assert_eq!(listed_bytes("15 datasets"), None);
    }

    #[test]
    fn a_resource_without_a_size_leaves_the_record_unsized() {
        let row = serde_json::json!({
            "resources": [{"size": 2_436_991_241_u64}, {"size": 0}]
        });
        assert_eq!(resource_bytes(&row), None);
        assert_eq!(
            resource_bytes(&serde_json::json!({"resources": []})),
            None
        );
    }
}
