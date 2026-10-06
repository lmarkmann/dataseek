//! Synapse (Sage Bionetworks) entity search, asked twice: once for entities
//! typed `dataset`, then for `project`s, where most Synapse data still lives.
//! Datasets first, projects fill the rest of the limit.

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, date_from_epoch, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    let mut found = Vec::new();
    for node_type in ["dataset", "project"] {
        let remaining = limit.saturating_sub(found.len());
        if remaining == 0 {
            break;
        }
        let body = ctx
            .http
            .post("https://repo-prod.prod.sagebase.org/repo/v1/search")
            .json_body(json!({
                "queryTerm": terms,
                "size": remaining,
                "booleanQuery": [{"key": "node_type", "value": node_type}],
            }))
            .json()?;
        if body.get("hits").is_none() && body.get("found").is_none() {
            return Err(SourceError::shape("no hits"));
        }
        found.extend(
            items(&body, "/hits").iter().filter_map(record).take(remaining),
        );
    }
    Ok(found)
}

fn record(hit: &Value) -> Option<Dataset> {
    let id = text(hit, "/id")?;
    let mut dataset = Dataset::new(
        &text(hit, "/name")?,
        &format!("https://www.synapse.org/Synapse:{id}"),
    )
    .describe(text(hit, "/description"));
    dataset.updated = number(hit, "/modified_on").and_then(date_from_epoch);
    dataset.valid()
}
