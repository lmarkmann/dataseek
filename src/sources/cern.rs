//! CERN Open Data (Invenio), limited to records of type Dataset.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://opendata.cern.ch/api/records/")
        .query("q", query)
        .query("type", "Dataset")
        .query("size", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/hits/hits").is_none() {
        return Err(SourceError::shape("no hits.hits"));
    }
    Ok(items(body, "/hits/hits")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
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
                aliases: vec![],
            }
        );
    }
}
