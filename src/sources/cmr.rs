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
//! Clients are asked to name themselves in `Client-Id`. Throttling has no
//! published numbers: a client over a rule gets 429 with `retry-after`
//! (CMR search API, October 2026).
//!
//! The link is the provider's "DATA SET LANDING PAGE" from `RelatedUrls`
//! (a doi.org link or the archive's page), else the DOI, else CMR's own
//! HTML view of the record. The license is the `UseConstraints.LicenseURL`
//! link, the only machine form UMM-C has; most records give free text
//! instead, which is not a license id. The size is the collection total
//! from `ArchiveAndDistributionInformation`, in decimal units; the schema
//! names KB to PB without saying which (UMM-C 1.18.6, October 2026). The
//! usage score CMR ranks by is not in any search response, only in a
//! separate whole-catalog table.
//! The formats are each `FileDistributionInformation` entry's `Format`, free
//! text such as "HDF5", "netCDF-4" or "HDF-EOS5" (UMM-C 1.18.6, probes,
//! October 2026).
//!
//! `--updated-since` goes out as `updated_since`, which compares the
//! revision date (CMR search API, October 2026). In 200 "sea ice" records
//! no `DataDates` update was later than the revision, so it drops nothing
//! the filter on the merged list keeps; it cut that query from 3,387
//! matches to 957 (probes, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let mut call = ctx
        .http
        .get("https://cmr.earthdata.nasa.gov/search/collections.umm_json")
        .header("Client-Id", "dataseek")
        .query("keyword", keyword(query))
        .query("page_size", limit.clamp(1, 2000));
    if let Some(day) = ctx.since {
        call = call.query("updated_since", format!("{day}T00:00:00Z"));
    }
    let body = call.json()?;
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
    let doi = text(item, "/umm/DOI/DOI").and_then(|raw| doi(&raw));
    let url = landing_page(item)
        .or_else(|| doi.as_ref().map(|doi| format!("https://doi.org/{doi}")))
        .unwrap_or_else(|| {
            format!("https://cmr.earthdata.nasa.gov/search/concepts/{id}.html")
        });
    let mut dataset = Dataset::new(&text(item, "/umm/EntryTitle")?, &url)
        .describe(text(item, "/umm/Abstract"));
    dataset.doi = doi;
    dataset.license = text(item, "/umm/UseConstraints/LicenseURL/Linkage");
    dataset.size_bytes = total_size(item);
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
    dataset.formats = items(
        item,
        "/umm/ArchiveAndDistributionInformation/FileDistributionInformation",
    )
    .iter()
    .filter_map(|file| text(file, "/Format"))
    .collect();
    dataset.valid()
}

/// The first landing page that is a plain web link; some records put a
/// bare DOI there.
fn landing_page(item: &Value) -> Option<String> {
    items(item, "/umm/RelatedUrls")
        .iter()
        .filter(|link| {
            link.get("Type")
                .is_some_and(|kind| kind == "DATA SET LANDING PAGE")
        })
        .filter_map(|link| text(link, "/URL"))
        .find(|url| {
            url.starts_with("http") && !url.contains(char::is_whitespace)
        })
}

/// The collection's total size, archive figures before distribution ones.
fn total_size(item: &Value) -> Option<u64> {
    let info = item.pointer("/umm/ArchiveAndDistributionInformation")?;
    items(info, "/FileArchiveInformation")
        .iter()
        .chain(items(info, "/FileDistributionInformation"))
        .find_map(|file| {
            let size = file.get("TotalCollectionFileSize")?.as_f64()?;
            let unit =
                match file.get("TotalCollectionFileSizeUnit")?.as_str()? {
                    "KB" => 1e3,
                    "MB" => 1e6,
                    "GB" => 1e9,
                    "TB" => 1e12,
                    "PB" => 1e15,
                    _ => return None,
                };
            (size > 0.0).then(|| bytes((size * unit).round()))
        })
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive approximate size; whole bytes are the intent"
)]
fn bytes(size: f64) -> u64 {
    size as u64
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
                url: "https://doi.org/10.5067/atlas/atl10.007".into(),
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
                formats: vec!["HDF5".into()],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn license_size_and_landing_page_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("cmr.tempo.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "TEMPO NO2 tropospheric and stratospheric columns V04 \
                        (PROVISIONAL)"
                    .into(),
                url: "https://doi.org/10.5067/IS-40e/TEMPO/NO2_L2.004".into(),
                description: Some(
                    "Nitrogen dioxide Level 2 (PROVISIONAL) files provide \
                     trace gas information at TEMPO\u{2019}s native spatial \
                     resolution, ~10 km^2 at the center of the Field of \
                     Regard (FOR), for individual granules. Each granule \
                     covers the entire North-South TEMPO FOR but only a \
                     portion of the East-West FOR."
                        .into()
                ),
                publisher: Some("NASA/LARC/SD/ASDC".into()),
                doi: Some("10.5067/is-40e/tempo/no2_l2.004".into()),
                license: Some(
                    "https://www.earthdata.nasa.gov/engage/\
                     open-data-services-software-policies/data-use-guidance"
                        .into()
                ),
                updated: Some("2026-10-16".into()),
                size_bytes: Some(15_000_000_000_000),
                popularity: None,
                formats: vec!["netCDF-4".into()],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_size_is_the_collection_total_in_decimal_units() {
        let hits = parse(&fixture::json("cmr.tempo.json"), 10).unwrap();
        let sizes: Vec<_> = hits.iter().map(|h| h.size_bytes).collect();
        assert_eq!(
            sizes,
            [
                Some(15_000_000_000_000),
                Some(12_000_000_000_000),
                Some(247_000_000_000),
                Some(175_000_000_000),
            ]
        );
        let hits = parse(&fixture::json("cmr.json"), 10).unwrap();
        assert!(hits.iter().all(|h| h.size_bytes.is_none()));
    }

    #[test]
    fn the_link_is_the_landing_page_else_the_doi_else_the_cmr_page() {
        let tempo = parse(&fixture::json("cmr.tempo.json"), 10).unwrap();
        let older = parse(&fixture::json("cmr.json"), 10).unwrap();
        let urls: Vec<_> =
            tempo.iter().chain(&older).map(|h| h.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://doi.org/10.5067/IS-40e/TEMPO/NO2_L2.004",
                "https://doi.org/10.5067/IS-40e/TEMPO/NO2_L3.004",
                "https://doi.org/10.5067/IS-40e/TEMPO/NO2_NRT_L2.002",
                "https://doi.org/10.5067/is-40e/tempo/no2_nrt_l3.002",
                "https://doi.org/10.5067/atlas/atl10.007",
                "https://doi.org/10.5067/amsru/au_si12_nrt_r04",
                "https://doi.org/10.26179/5d37f84cbf569",
                "https://cmr.earthdata.nasa.gov/search/concepts/\
                 C2545372296-AMD_USAPDC.html",
            ]
        );
    }

    #[test]
    fn only_a_license_link_counts_as_a_license() {
        let hits = parse(&fixture::json("cmr.json"), 10).unwrap();
        assert!(hits.iter().all(|h| h.license.is_none()));
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
