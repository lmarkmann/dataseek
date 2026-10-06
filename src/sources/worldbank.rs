//! World Bank indicators (29,533 rows for 29,490 codes, World Bank, October
//! 2026), downloaded in one request and searched locally; the indicator API
//! has no search parameter (`q`, `search` and `name` are ignored). `per_page`
//! has no documented maximum, and 40,000 returns the whole list in one page.
//! Descriptions are cut short to keep the cached catalog small.
//!
//! Only World Development Indicators (source 2) have a page on
//! `data.worldbank.org`, and 9 of 30 sampled ones answered HTTP 5xx; none of
//! 16 sampled indicators from the other databases opened a page, so those
//! link to their record in the API instead (World Bank, October 2026). No
//! rate limit is documented; the terms bar use that "exceeds reasonable
//! request volume or constitutes excessive or abusive usage" (World Bank
//! terms and conditions, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, shortened, text};

const INDICATORS: &str = "https://api.worldbank.org/v2/indicator";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(INDICATORS)
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
            let url = match text(row, "/source/id").as_deref() {
                Some("2") => {
                    format!("https://data.worldbank.org/indicator/{id}")
                }
                source => format!(
                    "{INDICATORS}/{id}?source={}&format=json",
                    source.unwrap_or_default()
                ),
            };
            let teaser = text(row, "/sourceNote")
                .and_then(|note| shortened(&note, 140));
            let mut dataset =
                Dataset::new(&text(row, "/name")?, &url).describe(teaser);
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
        assert_eq!(datasets.len(), 5);
        assert_eq!(
            datasets[0],
            Dataset {
                title: "Poverty Headcount ($1.90 a day)".into(),
                url: "https://api.worldbank.org/v2/indicator/\
                      1.0.HCount.1.90usd?source=37&format=json"
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
        assert_eq!(
            datasets[4],
            Dataset {
                title: "Population, total".into(),
                url: "https://data.worldbank.org/indicator/SP.POP.TOTL".into(),
                description: Some(
                    "Total population is based on the de facto definition of \
                     population, which counts all residents regardless of \
                     legal status or citizenship..."
                        .into()
                ),
                publisher: Some("World Development Indicators".into()),
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn an_error_answer_is_a_shape_error_never_an_empty_list() {
        let body = serde_json::json!([{"message": [{
            "id": "120",
            "key": "Invalid value",
            "value": "The provided parameter value is not valid",
        }]}]);
        assert!(matches!(parse(&body), Err(SourceError::Shape(_))));
    }
}
