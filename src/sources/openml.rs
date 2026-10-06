//! OpenML's active datasets, downloaded whole and searched locally: the REST
//! API filters by name and tag but has no full-text search. Only the latest
//! version of each name is kept.

use std::collections::HashMap;

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, number, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://www.openml.org/api/v1/json/data/list/limit/20000/status/active")
        .slow()
        .json()?;
    parse(&body)
}

pub(super) fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = items(body, "/data/dataset");
    if rows.is_empty() {
        return Err(SourceError::shape("no data.dataset list"));
    }
    let mut latest: HashMap<String, (u64, Dataset)> = HashMap::new();
    for row in rows {
        let (Some(id), Some(name)) = (number(row, "/did"), text(row, "/name"))
        else {
            continue;
        };
        let version = number(row, "/version").unwrap_or(0);
        if latest.get(&name).is_some_and(|(v, _)| *v >= version) {
            continue;
        }
        let quality = |key: &str| {
            items(row, "/quality").iter().find_map(|q| {
                (text(q, "/name").as_deref() == Some(key))
                    .then(|| number(q, "/value"))
                    .flatten()
            })
        };
        let shape = match (
            quality("NumberOfInstances"),
            quality("NumberOfFeatures"),
        ) {
            (Some(rows), Some(cols)) => {
                Some(format!("{rows} rows, {cols} features"))
            }
            _ => None,
        };
        let dataset =
            Dataset::new(&name, &format!("https://www.openml.org/d/{id}"))
                .describe(shape);
        latest.insert(name, (version, dataset));
    }
    Ok(latest
        .into_values()
        .map(|(_, d)| d)
        .filter_map(Dataset::valid)
        .collect())
}
