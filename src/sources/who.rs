//! The WHO Global Health Observatory's indicator list (OData), searched
//! locally. Links point at the indicator's OData data endpoint, the stable
//! address WHO publishes for each code.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://ghoapi.azureedge.net/api/Indicator")
        .slow()
        .json()?;
    Ok(parse(&body))
}

pub(super) fn parse(body: &Value) -> Vec<Dataset> {
    items(body, "/value")
        .iter()
        .filter_map(|row| {
            let code = text(row, "/IndicatorCode")?;
            let mut dataset = Dataset::new(
                &text(row, "/IndicatorName")?,
                &format!("https://ghoapi.azureedge.net/api/{code}"),
            );
            dataset.publisher = Some("World Health Organization".to_owned());
            dataset.valid()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let datasets = parse(&fixture::json("who.json"));
        assert_eq!(datasets.len(), 4);
        assert_eq!(
            datasets[0],
            Dataset {
                title: "Underweight among adults, BMI < 18.5 kg/m2 (crude \
                        estimate) (%)"
                    .into(),
                url: "https://ghoapi.azureedge.net/api/NCD_BMI_18C".into(),
                description: None,
                publisher: Some("World Health Organization".into()),
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
