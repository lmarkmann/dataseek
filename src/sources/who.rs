//! The WHO Global Health Observatory's indicator list (OData), searched
//! locally. Links point at the indicator's OData data endpoint, the stable
//! address WHO publishes for each code.

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://ghoapi.azureedge.net/api/Indicator")
        .slow()
        .json()?;
    Ok(items(&body, "/value")
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
        .collect())
}
