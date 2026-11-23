//! DataCite DOI search, restricted to resource type Dataset. One adapter that
//! reaches the DOI-minting repositories at once (Zenodo, Figshare, Dryad,
//! Dataverse installations, ICPSR, UKDS, GESIS, Pangaea, ...).
//!
//! Two kinds of noise are handled here rather than left to the user: DOIs
//! minted per machine event (GBIF user downloads, CCDC crystal structures)
//! are dropped, and every version DOI carries its
//! concept DOI as an alias so `dedup` folds versions into one hit. The page
//! asks for twice the limit to leave room for what is dropped, and when that
//! still leaves the list short the next page is read, up to [`MAX_PAGES`].
//!
//! Matches come back by relevance only when asked: without `sort=relevance`
//! the most recently updated come first, although the documentation names
//! relevance the default. A page holds up to 1,000 records and page numbers
//! reach 10,000 records. The API allows 1,000 requests per 5 minutes per IP
//! with a contact address and 500 without, and answers 429 beyond that
//! (DataCite, October 2026).
//!
//! The query is OpenSearch query string syntax. One it cannot parse comes
//! back as HTTP 400 or 500, the 500 being what marks a source as down, so it
//! is asked once more with its operators taken out (October 2026).
//!
//! `downloadCount` is what repositories report in usage reports, so zero
//! means nothing was reported, not that nothing was downloaded: 20 of 240
//! records over eight queries carried one, nearly all from Mendeley Data,
//! Harvard Dataverse and Dryad (October 2026). Only a positive count is the
//! popularity. Descriptions are often HTML escaped once more (`&lt;p&gt;`),
//! so they are cleaned twice.

use serde_json::Value;

use super::Ctx;
use crate::http::{CONTACT, SourceError};
use crate::record::{Dataset, clean, day, doi, items, number, text};

/// Dataset DOIs that are machine events, not datasets a person would look
/// for: one per CCDC crystal structure (1.26M) and one per GBIF user
/// download (most of gbif.gbif's 4.73M; its real datasets stay). Counts
/// measured 2026-10-06.
const NOISE_CLIENTS: &[&str] = &["ccdc.csd"];
const GBIF_DOWNLOADS: &str = "10.15468/dl.";

/// The largest `page[size]` the API documents.
const PAGE_CAP: usize = 1000;

/// A query that is nearly all noise would otherwise be read page after page.
const MAX_PAGES: usize = 5;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let size = limit.saturating_mul(2).clamp(1, PAGE_CAP);
    let (query, first) = super::retry_without_syntax(query, |query| {
        fetch(ctx, query, size, 1)
    })?;
    super::paginate_next(ctx, limit, MAX_PAGES, first, parse, |page| {
        fetch(ctx, &query, size, page)
    })
}

fn fetch(
    ctx: &Ctx<'_>,
    query: &str,
    size: usize,
    page: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get("https://api.datacite.org/dois")
        .query("query", query)
        .query("resource-type-id", "dataset")
        .query("sort", "relevance")
        .query("page[size]", size)
        .query("page[number]", page)
        .query("affiliation", "false")
        .query("mailto", CONTACT)
        .json()
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/data", limit, |row| {
        let client = text(row, "/relationships/client/data/id");
        let id = text(row, "/attributes/doi").unwrap_or_default();
        let noise = client
            .is_some_and(|c| NOISE_CLIENTS.contains(&c.as_str()))
            || id.to_lowercase().starts_with(GBIF_DOWNLOADS);
        if noise { None } else { record(row) }
    })
}

fn record(row: &Value) -> Option<Dataset> {
    let attributes = row.get("attributes")?;
    let id = text(attributes, "/doi")?;
    let url = text(attributes, "/url")
        .unwrap_or_else(|| format!("https://doi.org/{id}"));
    let title = text(attributes, "/titles/0/title")?;
    let abstract_text = items(attributes, "/descriptions")
        .iter()
        .find(|d| {
            d.get("descriptionType").and_then(Value::as_str)
                == Some("Abstract")
        })
        .or_else(|| items(attributes, "/descriptions").first())
        .and_then(|d| text(d, "/description"))
        .map(|t| clean(&t));
    let mut dataset =
        Dataset::new(&title, &url).describe(abstract_text).doi_from(Some(id));
    dataset.publisher = text(attributes, "/publisher")
        .or_else(|| text(attributes, "/publisher/name"));
    dataset.license = items(attributes, "/rightsList").iter().find_map(|r| {
        text(r, "/rightsIdentifier").or_else(|| text(r, "/rights"))
    });
    dataset.updated = day(text(attributes, "/updated"));
    dataset.popularity =
        number(attributes, "/downloadCount").filter(|&count| count > 0);
    dataset.aliases = items(attributes, "/relatedIdentifiers")
        .iter()
        .filter(|r| {
            r.get("relationType").and_then(Value::as_str)
                == Some("IsVersionOf")
        })
        .filter_map(|r| text(r, "/relatedIdentifier").as_deref().and_then(doi))
        .collect();
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("datacite.json"), 10).unwrap();
        assert_eq!(hits.len(), 6);
        assert_eq!(
            hits[2],
            Dataset {
                title: "Extracted Data From: National Emissions Inventory & \
                        Air Emissions"
                    .into(),
                url: "https://dataverse.harvard.edu/citation?persistentId=\
                      doi:10.7910/DVN/XUUTMY"
                    .into(),
                description: Some(
                    "This submission includes publicly available data \
                     extracted in its original form. Welcome to the \
                     one-stop shop for NEI-based data, organized by \
                     Inventory Year. Some inventory years do not include \
                     all of our data products."
                        .into()
                ),
                publisher: Some("Harvard Dataverse".into()),
                doi: Some("10.7910/dvn/xuutmy".into()),
                license: Some("cc0-1.0".into()),
                updated: Some("2026-04-21".into()),
                size_bytes: None,
                popularity: Some(14),
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_zero_count_is_no_popularity_and_a_version_names_its_concept() {
        let hits = parse(&fixture::json("datacite.json"), 10).unwrap();
        assert_eq!(hits[0].popularity, None);
        assert_eq!(hits[3].popularity, None);
        assert_eq!(hits[3].aliases, ["10.17632/y6p778djzj"]);
        assert_eq!(hits[4].popularity, Some(19));
        assert_eq!(hits[4].aliases, Vec::<String>::new());
    }

    #[test]
    fn gbif_downloads_are_dropped_and_its_datasets_stay() {
        let hits = parse(&fixture::json("datacite.noise.json"), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].title,
            "Vector arthropod occurrence records in \
                                   Georgia (Country)"
        );
        assert_eq!(hits[0].doi.as_deref(), Some("10.15468/dd.ube4vd"));
    }

    #[test]
    fn only_a_page_with_a_next_link_is_followed() {
        assert!(super::super::has_next(&fixture::json("datacite.json")));
        assert!(!super::super::has_next(
            &serde_json::json!({ "data": [], "links": {} })
        ));
    }
}
