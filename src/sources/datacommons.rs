//! Google Data Commons: statistical variables resolved from the query by the
//! v2 `resolve` endpoint's indicator resolver. Without a key of the user's
//! own, the trial key Data Commons publishes for general public use is sent;
//! it is quota-limited, so a personal key is the better route.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

/// Published at https://docs.datacommons.org/api/rest/v2/ for anyone to try
/// the API with; it is not a secret.
const TRIAL_KEY: &str = "AIzaSyCTI4Xz-UW_G2Q2RfknhcfdAnTHq5X5XuI";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let key = ctx
        .creds
        .get(Key::DataCommons)
        .map_or(TRIAL_KEY, |secret| secret.token());
    let body = ctx
        .http
        .get("https://api.datacommons.org/v2/resolve")
        .header("X-API-Key", key)
        .query("nodes", query)
        .query("resolver", "indicator")
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("entities").is_none() {
        return Err(SourceError::shape("no entities array"));
    }
    Ok(items(body, "/entities/0/candidates")
        .iter()
        .filter(|c| {
            items(c, "/typeOf")
                .iter()
                .any(|t| t.as_str() == Some("StatisticalVariable"))
        })
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(candidate: &Value) -> Option<Dataset> {
    let dcid = text(candidate, "/dcid")?;
    let name = text(candidate, "/name").unwrap_or_else(|| dcid.clone());
    let mut dataset = Dataset::new(
        &name,
        &format!("https://datacommons.org/browser/{dcid}"),
    )
    .describe(Some(dcid));
    dataset.publisher = Some("Data Commons".to_owned());
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("datacommons.json"), 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "unemployment rate".into(),
                url: "https://datacommons.org/browser/UnemploymentRate_Person"
                    .into(),
                description: Some("UnemploymentRate_Person".into()),
                publisher: Some("Data Commons".into()),
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
