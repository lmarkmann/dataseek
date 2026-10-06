//! The UCI Machine Learning Repository's full list (names only), searched
//! locally. The list endpoint is the one the site itself uses.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, number, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://archive.ics.uci.edu/api/datasets/list")
        .slow()
        .json()?;
    Ok(parse(&body))
}

pub(super) fn parse(body: &Value) -> Vec<Dataset> {
    items(body, "/data")
        .iter()
        .filter_map(|row| {
            let id = number(row, "/id")?;
            Dataset::new(
                &text(row, "/name")?,
                &format!("https://archive.ics.uci.edu/dataset/{id}"),
            )
            .valid()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::json("uci.json"));
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Abalone".into(),
                url: "https://archive.ics.uci.edu/dataset/1".into(),
                description: None,
                publisher: None,
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
