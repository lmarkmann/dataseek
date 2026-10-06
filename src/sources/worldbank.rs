//! World Bank indicators (about 29,500), downloaded in one request and
//! searched locally; the indicator API has no search parameter. Descriptions
//! are cut short to keep the cached catalog small.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, shortened, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.worldbank.org/v2/indicator")
        .query("format", "json")
        .query("per_page", 40_000)
        .slow()
        .json()?;
    parse(&body)
}

pub(super) fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = items(body, "/1");
    if rows.is_empty() {
        return Err(SourceError::shape("no indicator list"));
    }
    Ok(rows
        .iter()
        .filter_map(|row| {
            let id = text(row, "/id")?;
            let teaser = text(row, "/sourceNote")
                .and_then(|note| shortened(&note, 140));
            let mut dataset = Dataset::new(
                &text(row, "/name")?,
                &format!("https://data.worldbank.org/indicator/{id}"),
            )
            .describe(teaser);
            dataset.publisher = text(row, "/source/value");
            dataset.valid()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let datasets = parse(&fixture::json("worldbank.json")).unwrap();
        assert_eq!(datasets.len(), 4);
        assert_eq!(
            datasets[0],
            Dataset {
                title: "Poverty Headcount ($1.90 a day)".into(),
                url: "https://data.worldbank.org/indicator/1.0.HCount.1.90usd"
                    .into(),
                description: Some(
                    "The poverty headcount index measures the proportion of \
                     the population with daily per capita income (in 2011 \
                     PPP) below the poverty line."
                        .into()
                ),
                publisher: Some("LAC Equity Lab".into()),
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(
            datasets[2].description.as_deref(),
            Some(
                "The poverty gap captures the mean aggregate income or \
                 consumption shortfall relative to the poverty line across \
                 the entire population. It..."
            )
        );
    }
}
