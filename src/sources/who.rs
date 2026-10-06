//! The WHO Global Health Observatory's indicator list (OData), searched
//! locally. Links point at the indicator's OData data endpoint, the stable
//! address WHO publishes for each code.
//!
//! The list carries stubs named "Archived, see ..." or "See ..." that only
//! point at the indicator that replaced them; their data endpoint answers an
//! empty list. 36 of the 3,099 indicators are stubs (GHO OData API, October
//! 2026), and they are dropped.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

const STUB_PREFIXES: [&str; 2] = ["Archived, see ", "See "];

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
            let name = text(row, "/IndicatorName")?;
            if STUB_PREFIXES.iter().any(|stub| name.starts_with(stub)) {
                return None;
            }
            let mut dataset = Dataset::new(
                &name,
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
    fn records_map_from_a_recorded_list_without_its_stubs() {
        let datasets = parse(&fixture::json("who.json"));
        assert_eq!(datasets.len(), 4);
        assert_eq!(
            datasets[0],
            Dataset {
                title: "Percentage of health-care facilities with no access \
                        to any electricity supply (%)"
                    .into(),
                url: "https://ghoapi.azureedge.net/api/HCF_NO_ELECTRICITY"
                    .into(),
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
        assert_eq!(
            datasets[3].title,
            "Underweight among adults, BMI < 18.5 kg/m2 (crude estimate) (%)"
        );
    }
}
