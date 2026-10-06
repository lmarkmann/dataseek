//! NOAA's National Centers for Environmental Information, through its
//! dataset search service.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://www.ncei.noaa.gov/access/services/search/v1/datasets")
        .query("text", query)
        .query("limit", limit)
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
    let id = text(row, "/id")?;
    let landing = text(row, "/doiLink")
        .or_else(|| text(row, "/links/other/0/url"))
        .unwrap_or_else(|| {
            format!("https://www.ncei.noaa.gov/access/search/dataset-search?text={id}")
        });
    let mut dataset = Dataset::new(&text(row, "/name")?, &landing)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/doiLink"));
    dataset.publisher = Some("NOAA NCEI".to_owned());
    dataset.updated = text(row, "/endDate");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("ncei.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title:
                    "NOAA Optimum Interpolation 1/4 Degree Daily Sea Surface \
                        Temperature (OISST) Analysis, Version 2"
                        .into(),
                url: "https://doi.org/10.7289/V5SQ8XB5".into(),
                description: Some(
                    "This high-resolution sea surface temperature (SST) \
                     analysis product was developed using an optimum \
                     interpolation (OI) technique. The SST analysis has a \
                     spatial grid resolution of 0.25 (1/4) degree and \
                     temporal resolution of 1 day."
                        .into()
                ),
                publisher: Some("NOAA NCEI".into()),
                doi: Some("10.7289/v5sq8xb5".into()),
                license: None,
                updated: Some("2026-10-06".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
