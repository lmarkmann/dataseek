//! GitHub repositories tagged `dataset`, ranked by GitHub. Anonymous search
//! allows 10 requests a minute; a token raises that to 30 (GitHub, October
//! 2026).
//!
//! A page holds up to 100 repositories, which is the most `--per-source`
//! asks for, so one request answers every search. No query reaches past the
//! first 1,000 results: page 11 of 100 answers 422 (probed October 2026; the
//! docs say 4,000). A query over 256 characters is rejected (GitHub, October
//! 2026).
//!
//! An exhausted quota answers 403 with `x-ratelimit-remaining: 0` as well as
//! 429 (GitHub, October 2026); the HTTP client reports both as a rate limit.
//! `updated_at` moves when a repository is starred, so `pushed_at`, the last
//! push, is the update date (probed October 2026).

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
        call = call.key_header("Authorization", secret.authorization());
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
    dataset.updated = day(text(repo, "/pushed_at"));
    dataset.size_bytes =
        number(repo, "/size").and_then(|kib| kib.checked_mul(1024));
    dataset.popularity = number(repo, "/stargazers_count");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("github.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "mikejohnson51/climateR".into(),
                url: "https://github.com/mikejohnson51/climateR".into(),
                description: Some(
                    "An R \u{1f4e6} for getting point and gridded climate \
                     data by AOI"
                        .into()
                ),
                publisher: Some("mikejohnson51".into()),
                doi: None,
                license: None,
                updated: Some("2026-04-09".into()),
                size_bytes: Some(73_274_368),
                popularity: Some(202),
                aliases: vec![],
            }
        );
        assert_eq!(hits[3].license.as_deref(), Some("CC-BY-4.0"));
    }

    #[test]
    fn the_update_date_is_the_last_push_not_the_last_star() {
        let hits = parse(&fixture::json("github.json"), 10).unwrap();
        assert_eq!(hits[3].title, "RolnickLab/climart");
        assert_eq!(hits[3].updated.as_deref(), Some("2022-11-29"));
    }
}
