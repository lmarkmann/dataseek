//! DBnomics: official statistics from over 90 providers (IMF, OECD,
//! Eurostat, ECB, BIS, ILO, national statistics offices) behind one search.
//!
//! The index lists 94 providers (DBnomics, October 2026). A page holds up to
//! 100 datasets and a `limit` above that answers 400; `--per-source` stops at
//! 100 too, so one request answers every search, and `offset` pages further
//! (DBnomics API spec, October 2026). The search matches dataset names and
//! codes, not descriptions, and a query with no match returns an empty
//! `docs` list (probed October 2026). The API docs ask for no key and state
//! no rate limit; they recommend a cache on the client (DBnomics, October
//! 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.db.nomics.world/v22/search")
        .query("q", query)
        .query("limit", limit)
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/results/docs").is_none() {
        return Err(SourceError::shape("no results.docs"));
    }
    Ok(items(body, "/results/docs")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let provider = text(row, "/provider_code")?;
    let code = text(row, "/code")?;
    let series = number(row, "/nb_series");
    let mut dataset =
        Dataset::new(
            &text(row, "/name").unwrap_or_else(|| code.clone()),
            &format!("https://db.nomics.world/{provider}/{code}"),
        )
        .describe(text(row, "/description").or_else(|| {
            series.map(|n| format!("{n} series ({provider}/{code})"))
        }));
    dataset.publisher = text(row, "/provider_name");
    dataset.updated = day(text(row, "/updated_at"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn a_dataset_without_a_description_shows_its_series_count() {
        let row = json!({
            "code": "UNE_TUNE_SEX_AGE_EDU_NB",
            "name": "Unemployment by sex, age and education (thousands)",
            "nb_series": 230_497,
            "provider_code": "ILO",
        });
        assert_eq!(
            record(&row).unwrap().description.as_deref(),
            Some("230497 series (ILO/UNE_TUNE_SEX_AGE_EDU_NB)")
        );
    }

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("dbnomics.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Economic statistics ROPI-adjusted for inflation - \
                        Regions (for 'Developer API')"
                    .into(),
                url: "https://db.nomics.world/OECD/DSD_REG_ECO@DF_ECO_ROPI"
                    .into(),
                description: Some(
                    "This dataset provides indicators on real GDP, GVA and \
                     labour productivity measures in large regions (TL2) and \
                     small regions (TL3). Real values are deflation-adjusted \
                     using Regional Producer Price Index (ROPI), where \
                     available."
                        .into()
                ),
                publisher: Some(
                    "Organisation for Economic Co-operation and Development"
                        .into()
                ),
                doi: None,
                license: None,
                updated: Some("2026-06-12".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
