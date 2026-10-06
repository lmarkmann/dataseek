//! The UCI Machine Learning Repository's full list (names only), searched
//! locally. The list endpoint is the one the site itself uses.

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, number, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://archive.ics.uci.edu/api/datasets/list")
        .slow()
        .json()?;
    Ok(items(&body, "/data")
        .iter()
        .filter_map(|row| {
            let id = number(row, "/id")?;
            Dataset::new(
                &text(row, "/name")?,
                &format!("https://archive.ics.uci.edu/dataset/{id}"),
            )
            .valid()
        })
        .collect())
}
