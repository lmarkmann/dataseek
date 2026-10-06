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
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(body, "/results").iter().filter_map(record).take(limit).collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("openaire.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Replication Data for: In the Eye of the Storm: \
                        Hurricanes, Climate Migration, and Climate Attitudes"
                    .into(),
                url: "https://doi.org/10.7910/dvn/xptmgf".into(),
                description: Some(
                    "Abstract: Climate disasters raise the salience of \
                     climate change's negative consequences, including \
                     climate-induced migration."
                        .into()
                ),
                publisher: Some("Harvard Dataverse".into()),
                doi: Some("10.7910/dvn/xptmgf".into()),
                license: Some("CC 0".into()),
                updated: Some("2024-01-01".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec!["https://dx.doi.org/10.7910/dvn/xptmgf".into()],
            }
        );
    }
}
