//! DataCite DOI search, restricted to resource type Dataset. One adapter that
//! reaches the DOI-minting repositories at once (Zenodo, Figshare, Dryad,
//! Dataverse installations, ICPSR, UKDS, GESIS, Pangaea, ...).
//!
//! Two kinds of noise are handled here rather than left to the user: clients
//! that mint one Dataset DOI per machine event (GBIF per user download, CCDC
//! per crystal structure) are dropped, and every version DOI carries its
//! concept DOI as an alias so `dedup` folds versions into one hit. The page
//! asks for twice the limit to leave room for what is dropped.

use serde_json::Value;

use super::Ctx;
use crate::http::{CONTACT, SourceError};
use crate::record::{Dataset, day, doi, items, text};

/// Clients whose Dataset DOIs are machine events, not datasets a person
/// would look for. Counts measured 2026-10-06: gbif.gbif 4.73M, ccdc.csd
/// 1.26M.
const NOISE_CLIENTS: &[&str] = &["gbif.gbif", "ccdc.csd"];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://api.datacite.org/dois")
        .query("query", query)
        .query("resource-type-id", "dataset")
        .query("page[size]", limit.saturating_mul(2).clamp(1, 100))
        .query("affiliation", "false")
        .query("mailto", CONTACT)
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
    Ok(items(body, "/data")
        .iter()
        .filter(|row| {
            text(row, "/relationships/client/data/id")
                .is_none_or(|client| !NOISE_CLIENTS.contains(&client.as_str()))
        })
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let attributes = row.get("attributes")?;
    let id = text(attributes, "/doi")?;
    let url = text(attributes, "/url")
        .unwrap_or_else(|| format!("https://doi.org/{id}"));
    let title = text(attributes, "/titles/0/title")?;
    let abstract_text = items(attributes, "/descriptions")
        .iter()
        .find(|d| {
            d.get("descriptionType").and_then(Value::as_str)
                == Some("Abstract")
        })
        .or_else(|| items(attributes, "/descriptions").first())
        .and_then(|d| text(d, "/description"));
    let mut dataset =
        Dataset::new(&title, &url).describe(abstract_text).doi_from(Some(id));
    dataset.publisher = text(attributes, "/publisher")
        .or_else(|| text(attributes, "/publisher/name"));
    dataset.license = items(attributes, "/rightsList").iter().find_map(|r| {
        text(r, "/rightsIdentifier").or_else(|| text(r, "/rights"))
    });
    dataset.updated = day(text(attributes, "/updated"));
    dataset.aliases = items(attributes, "/relatedIdentifiers")
        .iter()
        .filter(|r| {
            r.get("relationType").and_then(Value::as_str)
                == Some("IsVersionOf")
        })
        .filter_map(|r| text(r, "/relatedIdentifier").as_deref().and_then(doi))
        .collect();
    dataset.valid()
}
