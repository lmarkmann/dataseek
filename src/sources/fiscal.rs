//! U.S. Treasury Fiscal Data: the Treasury's published datasets (debt,
//! receipts and outlays, auctions, interest rates), listed from the metadata
//! endpoint the site is built from and searched locally.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, first_text, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.fiscaldata.treasury.gov/services/dtg/metadata/")
        .slow()
        .json()?;
    parse(&body)
}

pub(super) fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of datasets"))?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let path = text(row, "/dataset_path")?;
            let mut dataset = Dataset::new(
                &text(row, "/title")?,
                &format!("https://fiscaldata.treasury.gov/datasets/{path}/"),
            )
            .describe(first_text(
                row,
                &["/short_description", "/summary", "/long_description"],
            ));
            dataset.publisher = text(row, "/publisher")
                .map(|office| format!("U.S. Treasury, {office}"));
            dataset.valid()
        })
        .collect())
}
