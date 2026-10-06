//! The OpenAIRE Graph, a deduplicated research graph harvested from DataCite,
//! Crossref and thousands of OAI-PMH repositories. Its value next to DataCite
//! is the repositories that never minted a DOI.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.openaire.eu/graph/v1/researchProducts")
        .query("search", query)
        .query("type", "dataset")
        .query("pageSize", limit.clamp(1, 100))
        .json()?;
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(&body, "/results")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let found_doi = items(row, "/pids").iter().find_map(|p| {
        (p.get("scheme").and_then(Value::as_str) == Some("doi"))
            .then(|| text(p, "/value").as_deref().and_then(doi))
            .flatten()
    });
    let instance_url = text(row, "/instances/0/urls/0");
    let url = match (&found_doi, &instance_url) {
        (Some(d), _) => format!("https://doi.org/{d}"),
        (None, Some(u)) => u.clone(),
        (None, None) => format!(
            "https://explore.openaire.eu/search/result?id={}",
            text(row, "/id")?
        ),
    };
    let mut dataset = Dataset::new(&text(row, "/mainTitle")?, &url)
        .describe(text(row, "/descriptions/0"));
    dataset.doi = found_doi;
    dataset.publisher = text(row, "/publisher");
    dataset.license = text(row, "/instances/0/license");
    dataset.updated = day(text(row, "/publicationDate"));
    if let Some(u) = instance_url {
        dataset.aliases.push(u);
    }
    dataset.valid()
}
