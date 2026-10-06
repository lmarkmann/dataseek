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
