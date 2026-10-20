//! data.europa.eu, the EU portal that harvests DCAT-AP from EU institutions
//! and the national and regional portals of member states. One adapter for
//! European government open data; fields arrive as language maps.
//!
//! Results come in relevance order. `filters=dataset` replaces the deprecated
//! `filter`, and a page holds up to 1000 datasets (hub-search OpenAPI 5.3.13,
//! October 2026). A hit carries every translation of its text and every
//! distribution, and the reply carries about 400 KB of facet counts, so the
//! request names the fields it reads with `includes`, which needs
//! `catalog.id` beside `catalog.title` to return the catalog, and turns the
//! counts off with `aggregation=false`; a reply for 10 datasets shrinks from
//! 750 KB to 150 KB (data.europa.eu, October 2026). An unbalanced `"` makes
//! the search engine fail with HTTP 400, while a balanced pair searches a
//! phrase and no other character matters (data.europa.eu, October 2026).
//! The DOI, where the record has one, is the link to doi.org that Zenodo
//! records carry as identifier or page and others as landing page, and a
//! Zenodo record's concept DOI arrives in `is_version_of` (probes, October
//! 2026). The metadata is CC0 and the API is read only; no rate limit,
//! caching term or API key is published (data.europa.eu legal notice, data
//! provider manual, October 2026).
//!
//! A license sits on each distribution. Where the portal maps it to the EU
//! licence vocabulary, `resource` is the spdx.org URI; otherwise `id`,
//! `label` and `resource` all repeat the publisher's own URI, most often a
//! Creative Commons deed (probes of 600 records, October 2026). `byte_size`
//! is per distribution, and data.gov.it writes 1024 or 0 for files of any
//! size: 1,098 of its 1,257 sized distributions in four queries (probes,
//! October 2026). The dataset's `modified`, else `issued`, is its date:
//! `catalog_record.modified` and a distribution's `modified` are the days
//! the portal re-harvested it, within the last few months even for records
//! published years before (probes, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, items, localized, number, text};

const INCLUDES: &str = "id,title,description,publisher,catalog.id,\
                        catalog.title,distributions.license,\
                        distributions.byte_size,modified,issued,\
                        landing_page,page,identifier,is_version_of";

/// The sizes data.gov.it writes for a file it has not measured.
const UNMEASURED_BYTES: [u64; 2] = [0, 1024];

/// License ids and URIs that say no license was given.
const NO_LICENSE: [&str; 2] = [
    "notspecified",
    "http://inspire.ec.europa.eu/metadata-codelist/\
     ConditionsApplyingToAccessAndUse/conditionsUnknown",
];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://data.europa.eu/api/hub/search/search")
        .query("q", balanced_quotes(query))
        .query("filters", "dataset")
        .query("includes", INCLUDES)
        .query("aggregation", "false")
        .query("limit", limit.clamp(1, 1000))
        .json()?;
    parse(&body, limit)
}

fn balanced_quotes(query: &str) -> String {
    if query.matches('"').count().is_multiple_of(2) {
        query.to_owned()
    } else {
        query.replace('"', " ")
    }
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/result/results").is_none() {
        return Err(SourceError::shape("no result.results"));
    }
    Ok(items(body, "/result/results")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let title = localized(row.get("title"))?;
    let mut dataset = Dataset::new(
        &title,
        &format!("https://data.europa.eu/data/datasets/{id}"),
    )
    .describe(localized(row.get("description")));
    dataset.doi = doi_of(row);
    dataset.publisher = row
        .pointer("/publisher/name")
        .and_then(|v| localized(Some(v)))
        .or_else(|| localized(row.pointer("/catalog/title")));
    let distributions = items(row, "/distributions");
    dataset.license = distributions.iter().find_map(license);
    dataset.updated =
        day(text(row, "/modified").or_else(|| text(row, "/issued")));
    dataset.size_bytes = distributions
        .iter()
        .filter_map(|d| number(d, "/byte_size"))
        .filter(|bytes| !UNMEASURED_BYTES.contains(bytes))
        .reduce(u64::saturating_add);
    dataset.aliases = items(row, "/is_version_of")
        .iter()
        .filter_map(|link| text(link, ""))
        .filter(|raw| raw.contains("doi.org/"))
        .filter_map(|raw| doi(&raw))
        .filter(|concept| dataset.doi.as_ref() != Some(concept))
        .collect();
    dataset.valid()
}

fn license(distribution: &Value) -> Option<String> {
    let license = distribution.get("license")?;
    let id = text(license, "/id");
    let resource = text(license, "/resource");
    if [&id, &resource]
        .into_iter()
        .flatten()
        .any(|named| NO_LICENSE.contains(&named.as_str()))
    {
        return None;
    }
    resource
        .as_deref()
        .and_then(spdx)
        .or_else(|| localized(license.get("label")))
        .or(id)
}

/// The SPDX id a license URI names: the id of an spdx.org URI, or the
/// license and version of an unported Creative Commons deed or legal code.
/// A national port such as `by/3.0/at` has no SPDX id of this form.
fn spdx(uri: &str) -> Option<String> {
    let path = uri.split_once("://")?.1;
    let path = path.strip_prefix("www.").unwrap_or(path);
    if let Some(id) = path.strip_prefix("spdx.org/licenses/") {
        let id = id.trim_end_matches('/').trim_end_matches(".html");
        return (!id.is_empty()).then(|| id.to_owned());
    }
    let mut parts = path.strip_prefix("creativecommons.org/")?.split('/');
    let (kind, code, version) = (parts.next()?, parts.next()?, parts.next()?);
    let rest = parts.next().unwrap_or_default();
    let unported = rest.is_empty()
        || rest.starts_with("legalcode")
        || rest.starts_with("deed");
    let numbered = version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_digit() || c == '.');
    if !unported || !numbered {
        return None;
    }
    match (kind, code) {
        ("publicdomain", "zero") => Some(format!("CC0-{version}")),
        (
            "licenses",
            "by" | "by-sa" | "by-nd" | "by-nc" | "by-nc-sa" | "by-nc-nd",
        ) => Some(format!("CC-{}-{version}", code.to_ascii_uppercase())),
        _ => None,
    }
}

fn doi_of(row: &Value) -> Option<String> {
    let links = ["/landing_page", "/page"]
        .into_iter()
        .flat_map(|pointer| items(row, pointer))
        .filter_map(|link| text(link, "/resource"));
    let names =
        items(row, "/identifier").iter().filter_map(|name| text(name, ""));
    links
        .chain(names)
        .filter(|raw| raw.contains("doi.org/"))
        .find_map(|raw| doi(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("europa.json"), 10).unwrap();
        assert_eq!(hits.len(), 6);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Combined SMOS and SMAP sea ice thickness Arctic"
                    .into(),
                url: "https://data.europa.eu/data/datasets/\
                      oai-zenodo-org-1631856"
                    .into(),
                description: Some(
                    "This data set contains Arctic sea ice thicknesses \
                     derived from L-band passive microwave brightness \
                     temperatures."
                        .into()
                ),
                publisher: Some("Zenodo".into()),
                doi: Some("10.5281/zenodo.1631856".into()),
                license: None,
                updated: Some("2024-08-01".into()),
                size_bytes: Some(43_125_616),
                popularity: None,
                aliases: vec!["10.5281/zenodo.1631855".into()],
            }
        );
    }

    #[test]
    fn a_doi_comes_from_a_doi_link_and_a_concept_doi_is_an_alias() {
        let hits = parse(&fixture::json("europa.json"), 10).unwrap();
        let dois: Vec<_> = hits.iter().map(|h| h.doi.as_deref()).collect();
        assert_eq!(
            dois,
            [
                Some("10.5281/zenodo.1631856"),
                Some("10.5281/zenodo.3540757"),
                None,
                None,
                None,
                Some("10.16904/envidat.381"),
            ]
        );
        assert_eq!(hits[1].aliases, ["10.5281/zenodo.3540756"]);
        assert_eq!(hits[5].aliases, Vec::<String>::new());
    }

    #[test]
    fn licenses_become_spdx_ids_where_the_uri_names_one() {
        let licenses = |name| {
            parse(&fixture::json(name), 10)
                .unwrap()
                .into_iter()
                .map(|h| h.license)
                .collect::<Vec<_>>()
        };
        let ogl = "http://reference.data.gov.uk/id/open-government-licence";
        let swiss = "http://dcat-ap.ch/vocabulary/licenses/terms_open";
        assert_eq!(
            licenses("europa.json"),
            [None, Some("CC-BY-4.0"), Some(ogl), Some("CC-BY-4.0"), Some(ogl)]
                .into_iter()
                .chain([Some(swiss)])
                .map(|l| l.map(String::from))
                .collect::<Vec<_>>()
        );
        let by = Some("CC-BY-4.0".to_owned());
        assert_eq!(
            licenses("europa.italy.json"),
            [
                by.clone(),
                None,
                None,
                None,
                by.clone(),
                by.clone(),
                by,
                Some("CC0-1.0".into()),
            ],
            "INSPIRE's conditionsUnknown is no license"
        );
    }

    #[test]
    fn a_national_creative_commons_port_keeps_its_uri() {
        assert_eq!(
            spdx("http://creativecommons.org/licenses/by/4.0/deed.nl")
                .as_deref(),
            Some("CC-BY-4.0")
        );
        assert_eq!(
            spdx("https://creativecommons.org/licenses/by/3.0/at/deed.de"),
            None
        );
        assert_eq!(spdx("http://dcat-ap.de/def/licenses/dl-by-de/2.0"), None);
    }

    #[test]
    fn sizes_add_up_and_data_gov_it_placeholders_are_dropped() {
        let sizes = |name| {
            parse(&fixture::json(name), 10)
                .unwrap()
                .into_iter()
                .map(|h| h.size_bytes)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            sizes("europa.json"),
            [Some(43_125_616), None, None, None, None, Some(2_386_070)]
        );
        assert_eq!(sizes("europa.italy.json"), [None; 8]);
    }

    #[test]
    fn the_date_is_the_last_modification_and_a_catalog_stands_in_as_publisher()
    {
        let hits = parse(&fixture::json("europa.italy.json"), 10).unwrap();
        let days: Vec<_> = hits.iter().map(|h| h.updated.as_deref()).collect();
        assert_eq!(
            days,
            [
                Some("2023-01-12"),
                None,
                None,
                Some("2025-06-13"),
                Some("2026-06-22"),
                Some("2020-01-16"),
                Some("2018-08-05"),
                Some("2019-09-30"),
            ]
        );
        assert_eq!(
            hits[1].publisher.as_deref(),
            Some("Italian Catalogue of metadata for Spatial Data")
        );
    }

    #[test]
    fn an_unbalanced_quote_is_dropped_and_a_phrase_is_kept() {
        assert_eq!(
            balanced_quotes("\"sea ice\" thickness"),
            "\"sea ice\" thickness"
        );
        assert_eq!(balanced_quotes("sea \"ice"), "sea  ice");
    }
}
