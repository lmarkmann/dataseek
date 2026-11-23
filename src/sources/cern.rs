//! CERN Open Data (Invenio), limited to records of type Dataset.
//!
//! Every record carries its whole file list and no parameter trims it: 100
//! records for "AOD" came to 141 MB in 21 s, past the client's body and time
//! limits, and one record alone reached 16 MB. Records are therefore fetched
//! ten to a request and the pages walked with `page` until the limit or the
//! last page. The API answers 60 requests a minute and then 429 with
//! `Retry-After: 60`, and stops at 10,000 results (CERN Open Data, October
//! 2026). A query its parser rejects comes back as HTTP 400 or 500, the 500
//! being what marks a source as down, so it is asked once more with its
//! operators taken out.
//!
//! `updated` is the publication year: the API's own `updated` is the same
//! day for all 100 "AOD" records, a bulk migration on 2025-06-06 (October
//! 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, number, text};

const PAGE: usize = 10;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let size = limit.clamp(1, PAGE);
    let (query, first) = super::retry_without_syntax(query, |query| {
        fetch(ctx, query, size, 1)
    })?;
    super::paginate_next(
        ctx,
        limit,
        limit.div_ceil(size),
        first,
        parse,
        |page| fetch(ctx, &query, size, page),
    )
}

fn fetch(
    ctx: &Ctx<'_>,
    query: &str,
    size: usize,
    page: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get("https://opendata.cern.ch/api/records/")
        .query("q", query)
        .query("type", "Dataset")
        .query("size", size)
        .query("page", page)
        .json()
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/hits/hits", limit, record)
}

fn record(hit: &Value) -> Option<Dataset> {
    let meta = hit.get("metadata")?;
    let recid = text(meta, "/recid").or_else(|| text(hit, "/id"))?;
    let mut dataset = Dataset::new(
        &text(meta, "/title")?,
        &format!("https://opendata.cern.ch/record/{recid}"),
    )
    .describe(text(meta, "/abstract/description"))
    .doi_from(text(meta, "/doi"));
    dataset.publisher = text(meta, "/experiment/0")
        .or_else(|| text(meta, "/experiment"))
        .map(|e| format!("CERN {e}"));
    dataset.license = text(meta, "/license/attribution");
    dataset.updated = text(meta, "/date_published");
    dataset.size_bytes = number(meta, "/distribution/size");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("cern.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "OPERA muon neutrino event 12316057896".into(),
                url: "https://opendata.cern.ch/record/4803".into(),
                description: Some(
                    "This OPERA muon neutrino event is a muon neutrino \
                     interaction with the lead target where a muon was \
                     reconstructed in the final state. The event data from \
                     Electronic Detectors are available in the Drift Tube, \
                     RPC, and Target Tracker files."
                        .into()
                ),
                publisher: Some("CERN OPERA".into()),
                doi: Some("10.7483/opendata.opera.ocjx.pjsn".into()),
                license: Some("CC0-1.0".into()),
                updated: Some("2018".into()),
                size_bytes: Some(8371),
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn only_a_page_with_a_next_link_is_followed() {
        assert!(super::super::has_next(&fixture::json("cern.json")));
        let empty = fixture::json("cern.empty.json");
        assert!(!super::super::has_next(&empty));
        assert_eq!(parse(&empty, 10).unwrap(), Vec::<Dataset>::new());
    }
}
