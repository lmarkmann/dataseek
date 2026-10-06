//! Data.gov's catalog. With a key (api.data.gov) it calls the documented
//! Catalog API on api.gsa.gov; without one it asks catalog.data.gov's own
//! search, which returns the same JSON keyless. Both replaced the CKAN API,
//! which data.gov retired in 2025.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let call = match ctx.creds.get(Key::DataGov) {
        Some(secret) => ctx
            .http
            .get("https://api.gsa.gov/technology/datagov/v4/search")
            .header("X-Api-Key", secret.token()),
        None => ctx.http.get("https://catalog.data.gov/search"),
    };
    let body = call.query("q", query).query("per_page", limit).json()?;
    let rows = body
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| SourceError::shape("no results array"))?;
    Ok(rows.iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let slug = text(row, "/slug")?;
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://catalog.data.gov/dataset/{slug}"),
    )
    .describe(text(row, "/description"))
    .doi_from(text(row, "/dcat/identifier"));
    dataset.publisher = text(row, "/dcat/publisher/name")
        .or_else(|| text(row, "/organization/name"));
    dataset.license = text(row, "/dcat/license");
    dataset.updated = day(text(row, "/dcat/modified"));
    if let Some(landing) = text(row, "/dcat/landingPage") {
        dataset.aliases.push(landing);
    }
    dataset.valid()
}
