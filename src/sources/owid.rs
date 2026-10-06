//! Our World in Data, through the search endpoint its site uses. Charts and
//! explorers only; articles are not datasets. The endpoint is not a
//! documented public API, so a shape change is expected one day and is
//! reported as such.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://ourworldindata.org/api/search")
        .query("q", query)
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
    Ok(items(body, "/results")
        .iter()
        .filter(|r| {
            matches!(
                r.get("type").and_then(Value::as_str),
                Some("chart" | "explorerView" | "multiDimView")
            )
        })
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let url = text(row, "/url").or_else(|| {
        text(row, "/slug")
            .map(|s| format!("https://ourworldindata.org/grapher/{s}"))
    })?;
    let mut dataset = Dataset::new(&text(row, "/title")?, &url)
        .describe(text(row, "/subtitle"));
    dataset.publisher = Some("Our World in Data".to_owned());
    dataset.license = Some("CC-BY-4.0".to_owned());
    dataset.updated = day(text(row, "/updatedAt"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("owid.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Share using safely managed drinking water".into(),
                url: "https://ourworldindata.org/grapher/\
                      proportion-using-safely-managed-drinking-water"
                    .into(),
                description: Some(
                    "Safely managed drinking water service means an \
                     improved water source is located on the premises, \
                     available when needed, and free from contamination."
                        .into()
                ),
                publisher: Some("Our World in Data".into()),
                doi: None,
                license: Some("CC-BY-4.0".into()),
                updated: Some("2026-05-11".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
