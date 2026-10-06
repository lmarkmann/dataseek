//! DBnomics: official statistics from about 90 providers (IMF, OECD,
//! Eurostat, ECB, BIS, ILO, national statistics offices) behind one search.

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
    let mut dataset = Dataset::new(
        &text(row, "/name").unwrap_or_else(|| code.clone()),
        &format!("https://db.nomics.world/{provider}/{code}"),
    )
    .describe(series.map(|n| format!("{n} series ({provider}/{code})")));
    dataset.publisher = text(row, "/provider_name");
    dataset.updated = day(text(row, "/updated_at"));
    dataset.valid()
}
