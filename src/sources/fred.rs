//! FRED (Federal Reserve Bank of St. Louis) series search. Requires the
//! user's free API key; the registry skips this source without one.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let secret =
        ctx.creds.get(Key::Fred).ok_or(SourceError::Unauthorized(401))?;
    let body = ctx
        .http
        .get("https://api.stlouisfed.org/fred/series/search")
        .query("search_text", query)
        .query("api_key", secret.token())
        .query("file_type", "json")
        .query("limit", limit.clamp(1, 1000))
        .json()?;
    if body.get("seriess").is_none() {
        return Err(SourceError::shape("no seriess array"));
    }
    Ok(items(&body, "/seriess")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let cadence = match (text(row, "/frequency"), text(row, "/units")) {
        (Some(f), Some(u)) => Some(format!("{f}, {u}")),
        (f, u) => f.or(u),
    };
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://fred.stlouisfed.org/series/{id}"),
    )
    .describe(text(row, "/notes").or(cadence));
    dataset.publisher = Some("Federal Reserve Bank of St. Louis".to_owned());
    dataset.updated = day(text(row, "/last_updated"));
    dataset.popularity = number(row, "/popularity");
    dataset.valid()
}
