//! Roboflow Universe, computer-vision datasets. Requires the user's Roboflow
//! API key. The response layout is documented only loosely, so the list is
//! looked for under the keys Roboflow's SDK reads, and each hit's fields
//! under their documented names; anything else is a shape error.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, first_text, number};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let secret =
        ctx.creds.get(Key::Roboflow).ok_or(SourceError::Unauthorized(401))?;
    let body = ctx
        .http
        .get("https://api.roboflow.com/universe/search")
        .query("q", query)
        .query("api_key", secret.token())
        .json()?;
    let hits = ["/results", "/data", "/projects", ""]
        .iter()
        .find_map(|p| body.pointer(p).and_then(Value::as_array))
        .ok_or_else(|| SourceError::shape("no result list"))?;
    Ok(hits.iter().filter_map(record).take(limit).collect())
}

fn record(hit: &Value) -> Option<Dataset> {
    let title = first_text(hit, &["/name", "/title"])?;
    let url = first_text(hit, &["/url", "/link"])?;
    let mut dataset = Dataset::new(&title, &url)
        .describe(first_text(hit, &["/description", "/type"]));
    dataset.license = first_text(hit, &["/license"]);
    dataset.popularity =
        number(hit, "/stars").or_else(|| number(hit, "/downloads"));
    dataset.valid()
}
