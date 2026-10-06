//! GitHub repositories tagged `dataset`, ranked by GitHub. Anonymous search
//! allows 10 requests a minute; a token raises that to 30.

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
    let mut call = ctx
        .http
        .get("https://api.github.com/search/repositories")
        .query("q", format!("{query} topic:dataset"))
        .query("per_page", limit.clamp(1, 100))
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(secret) = ctx.creds.get(Key::GitHub) {
        call = call.header("Authorization", secret.authorization());
    }
    let body = call.json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("items").is_none() {
        return Err(SourceError::shape("no items array"));
    }
    Ok(items(body, "/items").iter().filter_map(record).take(limit).collect())
}

fn record(repo: &Value) -> Option<Dataset> {
    let mut dataset =
        Dataset::new(&text(repo, "/full_name")?, &text(repo, "/html_url")?)
            .describe(text(repo, "/description"));
    dataset.publisher = text(repo, "/owner/login");
    dataset.license =
        text(repo, "/license/spdx_id").filter(|l| l != "NOASSERTION");
    dataset.updated = day(text(repo, "/updated_at"));
    dataset.popularity = number(repo, "/stargazers_count");
    dataset.valid()
}
