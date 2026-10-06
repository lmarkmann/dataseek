//! The OpenDataSoft federated hub (data.opendatasoft.com), which indexes the
//! public datasets of OpenDataSoft portals: French cities and regions,
//! utilities, Swiss and UK councils. Explore API v2.1, ODSQL `search()`.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let literal: String = query.chars().filter(|c| *c != '"').collect();
    let body = ctx
        .http
        .get("https://data.opendatasoft.com/api/explore/v2.1/catalog/datasets")
        .query("where", format!("search(\"{literal}\")"))
        .query("limit", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(body, "/results").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/dataset_id")?;
    let meta = row.pointer("/metas/default")?;
    let mut dataset = Dataset::new(
        &text(meta, "/title")?,
        &format!("https://data.opendatasoft.com/explore/dataset/{id}/"),
    )
    .describe(text(meta, "/description"));
    dataset.publisher = text(meta, "/publisher")
        .or_else(|| text(meta, "/source_domain_title"));
    dataset.license = text(meta, "/license");
    dataset.updated = day(text(meta, "/modified"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("opendatasoft.json"), 10).unwrap();
        assert_eq!(hits.len(), 5);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Nitrate concentration parameters in the water column \
                        | Concentration of nitrate {NO3} per unit volume of \
                        the water body [unknown phase] | EMODNet Chemistry 2 \
                        | Black Sea DIVA 4D analysis of Water_body_nitrate - \
                        Summer"
                    .into(),
                url: "https://data.opendatasoft.com/explore/dataset/\
                      nitrate-concentration-parameters-in-the-water-column-\
                      concentration-of-nitrate-no3-per-unit-volume-of-the-\
                      water-body-unknown-phase-emodnet-chemistry-2-black-sea-\
                      diva-4d-analysis-of-water_body_nitrate-summer@pndb/"
                    .into(),
                description: Some("Lien vers la fiche source".into()),
                publisher: Some("PNDB".into()),
                doi: None,
                license: None,
                updated: Some("2018-03-26".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
