//! Google Data Commons: statistical variables resolved from the query by the
//! v2 `resolve` endpoint's indicator resolver. Every request to
//! api.datacommons.org must carry a key, and the one the docs publish is a
//! trial for single requests, not for software (Data Commons, October 2026),
//! so the user's own key is required and the registry skips the source
//! without one. The key travels in the `X-API-Key` header, which the endpoint
//! accepts on GET (an invalid key is rejected as such, probed October 2026),
//! so it never reaches a URL. An indicator search returns every match in one
//! response, best score first, with no paging (Data Commons, October 2026);
//! topics are dropped. The resolver names a variable but sends no
//! description, so the record has none.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let secret = ctx
        .creds
        .get(Key::DataCommons)
        .ok_or(SourceError::Unauthorized(401))?;
    let body = ctx
        .http
        .get("https://api.datacommons.org/v2/resolve")
        .key_header("X-API-Key", secret.token())
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
    );
    dataset.publisher = Some("Data Commons".to_owned());
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{Services, fixture};

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
                description: None,
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

    #[test]
    fn without_the_users_key_the_source_refuses_before_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let outcome = search(&services.ctx(false), "population", 10);
        assert!(matches!(outcome, Err(SourceError::Unauthorized(401))));
    }
}
