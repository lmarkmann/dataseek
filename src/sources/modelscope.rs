//! ModelScope (Alibaba), the largest Chinese ML hub, through the dataset
//! listing its site and its own MCP server call without a token. It is not in
//! ModelScope's OpenAPI, whose dataset listing declares a bearer token and
//! pages of 50 (ModelScope, October 2026).
//!
//! A page holds up to 100 records and a larger `PageSize` is capped there, so
//! one request covers any `--per-source`. `StorageSize` is bytes, equal to the
//! sum of the file sizes in the repository tree, and 0 on records whose size
//! was never computed; `Size` is 0 on every record. `GmtModified` is a row
//! touch that differs from `LastUpdatedTime` on 299 of 300 records sampled, so
//! `updated` is `LastUpdatedTime`, which the OpenAPI calls `last_modified`.
//! `Description` is the owner's one-line summary and empty on about a third of
//! records; the Chinese display name is a title, so it is not used in its place
//! (ModelScope, October 2026).

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
    .describe(text(row, "/Description"));
    dataset.publisher = Some(namespace);
    dataset.license = text(row, "/License");
    dataset.updated =
        number(row, "/LastUpdatedTime").and_then(date_from_epoch);
    dataset.size_bytes =
        number(row, "/StorageSize").filter(|&bytes| bytes > 0);
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
                title: "modelscope/DIV2K".into(),
                url: "https://www.modelscope.cn/datasets/modelscope/DIV2K"
                    .into(),
                description: Some(
                    "DIV2K\u{56fe}\u{50cf}\u{8d85}\u{5206}\u{8fa8}\u{7387}\
                     \u{6570}\u{636e}\u{96c6}"
                        .into()
                ),
                publisher: Some("modelscope".into()),
                doi: None,
                license: Some("Apache License 2.0".into()),
                updated: Some("2023-03-01".into()),
                size_bytes: Some(68_276),
                popularity: Some(2275),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_missing_summary_or_size_stays_empty() {
        let hits = parse(&fixture::json("modelscope.json"), 10).unwrap();
        assert_eq!(hits[3].title, "chenasdf/DIV2K");
        assert_eq!(hits[3].description, None);
        assert_eq!(hits[3].size_bytes, None);
        assert_eq!(hits[3].updated.as_deref(), Some("2026-05-21"));
    }
}
