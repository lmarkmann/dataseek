//! NASA's Common Metadata Repository: earth-science collections from NASA's
//! data centers and partner agencies. Search is keyless; downloads need an
//! Earthdata Login, which dataseek never touches.
//!
//! The request asks for `umm_json`, the collection record itself. The plain
//! `json` format sends a date for one collection in four and a DOI only
//! where one of its links is a DOI resolver; `umm_json` has the DOI in
//! `DOI.DOI` and the date the provider last updated the data in `DataDates`,
//! with the record's last revision in `meta.revision-date` for the rest (CMR
//! search API, October 2026). `page_size` goes to 2000 and keyword searches
//! come back by relevance, so one request covers any limit and keeps the
//! source's order. CMR answers HTTP 400 to a keyword that mixes a quoted
//! phrase with words or leaves a quote open, which `keyword` prevents.
//! Clients are asked to name themselves in `Client-Id`.

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
        .get("https://cmr.earthdata.nasa.gov/search/collections.umm_json")
        .header("Client-Id", "dataseek")
        .query("keyword", keyword(query))
        .query("page_size", limit.clamp(1, 2000))
        .json()?;
    parse(&body, limit)
}

/// The query as CMR's keyword syntax takes it: words, or one quoted phrase.
/// Any other use of a quote is dropped so the words still match.
fn keyword(query: &str) -> String {
    let query = query.trim();
    let one_phrase = query
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .is_some_and(|phrase| !phrase.contains('"'));
    if one_phrase { query.to_owned() } else { query.replace('"', " ") }
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("items").is_none() {
        return Err(SourceError::shape("no items"));
    }
    Ok(items(body, "/items").iter().filter_map(record).take(limit).collect())
}

fn record(item: &Value) -> Option<Dataset> {
    let id = text(item, "/meta/concept-id")?;
    let mut dataset = Dataset::new(
        &text(item, "/umm/EntryTitle")?,
        &format!("https://cmr.earthdata.nasa.gov/search/concepts/{id}.html"),
    )
    .describe(text(item, "/umm/Abstract"))
    .doi_from(text(item, "/umm/DOI/DOI"));
    let centers = items(item, "/umm/DataCenters");
    let archive = centers.iter().find(|center| {
        items(center, "/Roles").iter().any(|role| role == "ARCHIVER")
    });
    dataset.publisher = archive
        .or(centers.first())
        .and_then(|center| text(center, "/ShortName"));
    let data_update = items(item, "/umm/DataDates")
        .iter()
        .find(|date| date.get("Type").is_some_and(|kind| kind == "UPDATE"))
        .and_then(|date| text(date, "/Date"));
    dataset.updated =
        day(data_update.or_else(|| text(item, "/meta/revision-date")));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("cmr.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "ATLAS/ICESat-2 L3A Sea Ice Freeboard V007".into(),
                url: "https://cmr.earthdata.nasa.gov/search/concepts/\
                      C3565574246-NSIDC_CPRD.html"
                    .into(),
                description: Some(
                    "ATL10 contains along-track sea ice freeboard calculated \
                     for 10 km swath segments. The data were acquired by the \
                     Advanced Topographic Laser Altimeter System (ATLAS) \
                     instrument on board the ICESat-2 observatory."
                        .into()
                ),
                publisher: Some("NASA NSIDC DAAC".into()),
                doi: Some("10.5067/atlas/atl10.007".into()),
                license: None,
                updated: Some("2026-10-02".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_doi_comes_from_the_records_own_field() {
        let hits = parse(&fixture::json("cmr.json"), 10).unwrap();
        let dois: Vec<_> = hits.iter().map(|h| h.doi.as_deref()).collect();
        assert_eq!(
            dois,
            [
                Some("10.5067/atlas/atl10.007"),
                Some("10.5067/amsru/au_si12_nrt_r04"),
                Some("10.26179/5d37f84cbf569"),
                None,
            ]
        );
    }

    #[test]
    fn the_publisher_is_the_archive_or_else_the_first_center() {
        let hits = parse(&fixture::json("cmr.json"), 10).unwrap();
        let publishers: Vec<_> =
            hits.iter().map(|h| h.publisher.as_deref()).collect();
        assert_eq!(
            publishers,
            [
                Some("NASA NSIDC DAAC"),
                Some("NASA/MSFC/AMSR SIPS/LANCE"),
                Some("AU/AADC"),
                Some("USAP-DC"),
            ]
        );
    }

    #[test]
    fn the_providers_update_date_wins_over_the_revision_date() {
        let hits = parse(&fixture::json("cmr.json"), 10).unwrap();
        let dates: Vec<_> =
            hits.iter().map(|h| h.updated.as_deref()).collect();
        assert_eq!(
            dates,
            [
                Some("2026-10-02"),
                Some("2020-06-30"),
                Some("2021-08-11"),
                Some("2022-05-16"),
            ]
        );
    }

    #[test]
    fn a_body_without_items_is_a_changed_response() {
        let body = serde_json::json!({"feed": {"entry": []}});
        assert!(matches!(parse(&body, 10), Err(SourceError::Shape(_))));
    }

    #[test]
    fn quotes_that_cmr_would_reject_are_dropped() {
        assert_eq!(keyword("sea ice"), "sea ice");
        assert_eq!(keyword(" \"sea ice\" "), "\"sea ice\"");
        assert_eq!(keyword("\"sea ice\" extent"), " sea ice  extent");
        assert_eq!(keyword("\"sea ice"), " sea ice");
        assert_eq!(keyword("\"sea\" \"ice\""), " sea   ice ");
    }
}
