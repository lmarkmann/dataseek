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
//! records carry as identifier or page and others as landing page. The
//! metadata is CC0 and the API is read only; no rate limit or caching term
//! is published (data.europa.eu legal notice, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, items, localized, text};

const INCLUDES: &str = "id,title,description,publisher,catalog.id,\
                        catalog.title,distributions.license,modified,issued,\
                        landing_page,page,identifier";

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
    dataset.license = items(row, "/distributions").iter().find_map(|d| {
        localized(d.pointer("/license/label"))
            .or_else(|| text(d, "/license/id"))
    });
    dataset.updated =
        day(text(row, "/modified").or_else(|| text(row, "/issued")));
    dataset.valid()
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
        assert_eq!(hits.len(), 5);
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
                license: Some(
                    "https://creativecommons.org/licenses/by/4.0/legalcode"
                        .into()
                ),
                updated: Some("2024-08-01".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_doi_comes_from_a_doi_link_and_a_catalog_stands_in_as_publisher() {
        let hits = parse(&fixture::json("europa.json"), 10).unwrap();
        let dois: Vec<_> = hits.iter().map(|h| h.doi.as_deref()).collect();
        assert_eq!(
            dois,
            [
                Some("10.5281/zenodo.1631856"),
                None,
                Some("10.1594/wdcc/uni_hh_mi_acsys2003"),
                Some("10.5281/zenodo.14975004"),
                Some("10.5281/zenodo.3540757"),
            ]
        );
        assert_eq!(hits[2].publisher.as_deref(), Some("GDI-DE"));
        assert_eq!(hits[1].updated.as_deref(), Some("2023-03-22"));
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
