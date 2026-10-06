//! OSF through SHARE's trove index. OSF has no "dataset" resource type, so
//! this searches projects and registrations, which is where OSF data lives.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

const TYPES: &str =
    "https://osf.io/vocab/2022/Project,https://osf.io/vocab/2022/Registration";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://share.osf.io/trove/index-card-search")
        .query("cardSearchText", query)
        .query("cardSearchFilter[resourceType]", TYPES)
        .query("page[size]", limit.clamp(1, 100))
        .query("acceptMediatype", "application/json")
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("data").is_none() {
        return Err(SourceError::shape("no data array"));
    }
    Ok(items(body, "/data").iter().filter_map(record).take(limit).collect())
}

fn record(card: &Value) -> Option<Dataset> {
    let mut dataset =
        Dataset::new(&text(card, "/title/0/@value")?, &text(card, "/@id")?)
            .describe(text(card, "/description/0/@value"));
    dataset.publisher = text(card, "/creator/0/name/0/@value")
        .or_else(|| text(card, "/publisher/0/name/0/@value"));
    dataset.updated = day(text(card, "/dateModified/0/@value"));
    dataset.valid()
}
