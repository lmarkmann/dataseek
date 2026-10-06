//! NADA, the microdata catalog software of the World Bank, IHSN, FAO, UNHCR
//! and many national statistics offices. Same catalog search everywhere; a
//! registry row per installation picks the base URL.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{base}/api/catalog/search"))
        .query("sk", query)
        .query("ps", limit)
        .json()?;
    if body.pointer("/result/rows").is_none() {
        return Err(SourceError::shape("no result.rows"));
    }
    Ok(items(&body, "/result/rows")
        .iter()
        .filter_map(|row| record(base, row))
        .take(limit)
        .collect())
}

fn record(base: &str, row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let url = text(row, "/url").unwrap_or_else(|| {
        format!("{}/catalog/{id}", base.trim_end_matches("/index.php"))
    });
    let coverage = match (
        text(row, "/nation"),
        number(row, "/year_start"),
        number(row, "/year_end"),
    ) {
        (Some(nation), Some(from), Some(to)) if from != to => {
            Some(format!("{nation}, {from}-{to}"))
        }
        (Some(nation), Some(year), _) => Some(format!("{nation}, {year}")),
        (nation, _, _) => nation,
    };
    let abstract_text = text(row, "/abstract").filter(|a| a != "null");
    let mut dataset = Dataset::new(&text(row, "/title")?, &url)
        .describe(abstract_text.or(coverage))
        .doi_from(text(row, "/doi"));
    dataset.publisher = text(row, "/authoring_entity");
    dataset.updated = day(text(row, "/changed"));
    dataset.popularity = number(row, "/total_downloads");
    dataset.valid()
}
