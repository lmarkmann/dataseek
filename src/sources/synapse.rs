//! Synapse (Sage Bionetworks) entity search, asked twice: once for entities
//! typed `dataset`, then for `project`s, where most Synapse data still lives.
//! Datasets first, projects fill the rest of the limit.
//!
//! The docs give `size` a default of 10 and no maximum; the server returned a
//! full page of 1,000 hits, so one request per type fills the 100 the CLI
//! allows and `start` is not needed (Synapse REST docs and live API, October
//! 2026). Without a token only public entities come back, and a search
//! answers 201. Several `queryTerm` words match any of them, ranked by
//! relevance, not all of them. The index also holds tables, views, dataset
//! collections (50), files and folders, which are not asked for (live API,
//! October 2026).

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{
    Dataset, date_from_epoch, from_markdown, items, number, text,
};

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
        found.extend(parse(&body, remaining)?);
    }
    Ok(found)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("hits").is_none() && body.get("found").is_none() {
        return Err(SourceError::shape("no hits"));
    }
    Ok(items(body, "/hits").iter().filter_map(record).take(limit).collect())
}

fn record(hit: &Value) -> Option<Dataset> {
    let id = text(hit, "/id")?;
    let mut dataset = Dataset::new(
        &text(hit, "/name")?,
        &format!("https://www.synapse.org/Synapse:{id}"),
    )
    .describe(text(hit, "/description").map(|d| from_markdown(&d)));
    dataset.updated = number(hit, "/modified_on").and_then(date_from_epoch);
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("synapse.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Multi-omic Glial Programs in Brain Aging".into(),
                url: "https://www.synapse.org/Synapse:syn75275226".into(),
                description: Some(
                    "Title: Aged brain multi-omic integration captures \
                     immunometabolic and sex variation Dataset contact: \
                     Justin P. Whalley"
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: Some("2026-06-08".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
