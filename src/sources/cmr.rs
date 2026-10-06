//! NASA's Common Metadata Repository: earth-science collections from NASA's
//! data centers and partner agencies. Search is keyless; downloads need an
//! Earthdata Login, which dataseek never touches.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://cmr.earthdata.nasa.gov/search/collections.json")
        .query("keyword", query)
        .query("page_size", limit.clamp(1, 2000))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/feed/entry").is_none() {
        return Err(SourceError::shape("no feed.entry"));
    }
    Ok(items(body, "/feed/entry")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(entry: &Value) -> Option<Dataset> {
    let id = text(entry, "/id")?;
    let mut dataset = Dataset::new(
        &text(entry, "/title")?,
        &format!("https://cmr.earthdata.nasa.gov/search/concepts/{id}.html"),
    )
    .describe(text(entry, "/summary"))
    .doi_from(
        items(entry, "/links")
            .iter()
            .filter_map(|link| text(link, "/href"))
            .find(|href| href.contains("doi.org/")),
    );
    dataset.publisher = first_text(
        entry,
        &["/archive_center", "/organizations/0", "/data_center"],
    );
    dataset.updated = day(text(entry, "/updated"));
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
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
