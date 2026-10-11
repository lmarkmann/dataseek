//! OpenML's active datasets, downloaded whole and searched locally: the REST
//! API filters by name and tag but has no full-text search (live API, October
//! 2026). Only the latest version of each name is kept, in the place its name
//! first appears.
//!
//! One request returns every active dataset: 6,434 against the 20,000 limit
//! in the URL (October 2026). Past that limit the list would be cut silently;
//! `limit/10000/offset/N` pages it, and a page past the end answers HTTP 412
//! with error 372 (OpenML, October 2026). The list holds a name, a version and
//! a few size qualities per dataset, no text, so the description is the size.
//! The real description is one request per dataset, 6,434 of them against
//! unpublished rate limits ("rate limits apply", docs.openml.org/intro).

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
    let mut latest: Vec<(u64, Dataset)> = Vec::new();
    let mut slot: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let (Some(id), Some(name)) = (number(row, "/did"), text(row, "/name"))
        else {
            continue;
        };
        let version = number(row, "/version").unwrap_or(0);
        let seen = slot.get(&name).copied();
        if seen.and_then(|i| latest.get(i)).is_some_and(|(v, _)| *v >= version)
        {
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
        if let Some(older) = seen.and_then(|i| latest.get_mut(i)) {
            *older = (version, dataset);
        } else {
            slot.insert(name, latest.len());
            latest.push((version, dataset));
        }
    }
    Ok(latest.into_iter().map(|(_, d)| d).filter_map(Dataset::valid).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::json("openml.json")).unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            ["anneal", "kr-vs-kp", "labor", "18ProductivityPrediction"]
        );
        assert_eq!(
            hits[3],
            Dataset {
                title: "18ProductivityPrediction".into(),
                url: "https://www.openml.org/d/43250".into(),
                description: Some("1197 rows, 15 features".into()),
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
