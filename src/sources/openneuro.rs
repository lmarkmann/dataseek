//! OpenNeuro's public datasets, paged out of its GraphQL API and searched
//! locally: the GraphQL `search` field returns nothing to anonymous clients.

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

const PAGE: &str = "query($after: String) { datasets(first: 100, after: $after, \
    filterBy: {public: true}) { pageInfo { hasNextPage endCursor } edges { node \
    { id latestSnapshot { created description { Name } } } } } }";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    let mut after = Value::Null;
    for _ in 0..60 {
        let body = ctx
            .http
            .post("https://openneuro.org/crn/graphql")
            .json_body(json!({"query": PAGE, "variables": {"after": after}}))
            .slow()
            .json()?;
        let (page, next) = parse(&body)?;
        entries.extend(page);
        match next {
            Some(cursor) => after = Value::String(cursor),
            None => break,
        }
    }
    Ok(entries)
}

/// One page of datasets and the cursor of the next page, if any.
pub(super) fn parse(
    body: &Value,
) -> Result<(Vec<Dataset>, Option<String>), SourceError> {
    let page = body
        .pointer("/data/datasets")
        .ok_or_else(|| SourceError::shape("no data.datasets"))?;
    let entries = items(page, "/edges")
        .iter()
        .filter_map(|edge| {
            let id = text(edge, "/node/id")?;
            let name = text(edge, "/node/latestSnapshot/description/Name")
                .unwrap_or_else(|| id.clone());
            let mut dataset = Dataset::new(
                &name,
                &format!("https://openneuro.org/datasets/{id}"),
            );
            dataset.updated = day(text(edge, "/node/latestSnapshot/created"));
            dataset.valid()
        })
        .collect();
    let more = page.pointer("/pageInfo/hasNextPage").and_then(Value::as_bool);
    let next = match (more, text(page, "/pageInfo/endCursor")) {
        (Some(true), Some(cursor)) => Some(cursor),
        _ => None,
    };
    Ok((entries, next))
}
