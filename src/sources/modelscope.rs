//! ModelScope (Alibaba), the largest Chinese ML hub, through the dataset
//! listing its site uses. Not a documented public API.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, date_from_epoch, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://www.modelscope.cn/api/v1/dolphin/datasets")
        .query("Query", query)
        .query("PageSize", limit.clamp(1, 100))
        .query("PageNumber", 1)
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("Data").is_none() {
        return Err(SourceError::shape("no Data array"));
    }
    Ok(items(body, "/Data").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let namespace = text(row, "/Namespace")?;
    let name = text(row, "/Name")?;
    let mut dataset = Dataset::new(
        &format!("{namespace}/{name}"),
        &format!("https://www.modelscope.cn/datasets/{namespace}/{name}"),
    )
    .describe(text(row, "/Description").or_else(|| text(row, "/ChineseName")));
    dataset.publisher = Some(namespace);
    dataset.license = text(row, "/License");
    dataset.updated = number(row, "/GmtModified").and_then(date_from_epoch);
    dataset.popularity = number(row, "/Downloads");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("modelscope.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "CarbonGPT/climate-sr".into(),
                url: "https://www.modelscope.cn/datasets/CarbonGPT/climate-sr"
                    .into(),
                description: Some(
                    "\u{6c14}\u{5019}\u{6570}\u{636e}\u{8d85}\u{5206}".into()
                ),
                publisher: Some("CarbonGPT".into()),
                doi: None,
                license: Some("Apache License 2.0".into()),
                updated: Some("2026-10-06".into()),
                size_bytes: None,
                popularity: Some(4618),
                aliases: vec![],
            }
        );
    }
}
