//! GitLab.com projects tagged `dataset`, the most starred first. The
//! projects API matches every query word as a substring of the project's
//! path, name or description, ignoring case, and offers a relevance order
//! only for the projects of a signed-in member, so the stars rank (GitLab
//! projects API docs, probed 2026-11-23).
//!
//! The group `data/d/openml` is a read-only mirror of OpenML with one
//! project per dataset, each tagged `dataset`: 14 of the 15 projects "mnist"
//! finds and 78 of the 86 for "weather" (probed 2026-11-23). dataseek
//! searches OpenML itself, so those projects are dropped. Every search asks
//! for a full page of 100, the most a page holds, so that the drop still
//! leaves `limit` projects where the page has them; one request answers a
//! search.
//!
//! No key is read: an anonymous answer holds every public project. Its rows
//! carry neither a license nor a size. `last_activity_at`, the update date,
//! moves on any project event, an issue or a comment as well as a push, at
//! most once an hour (GitLab projects API docs, 2026-11-23).
//!
//! Anonymous clients get 60 requests per IP in each clock hour. The 61st
//! answers 429 under `ratelimit-name: unauthenticated_traffic_per_ip`, with
//! `retry-after` set to the seconds left in the hour (observed 2026-11-23).
//! A response inside the limit names a wider one, `ratelimit-limit: 500`
//! under `throttle_unauthenticated_api`, and the docs give 400 project-list
//! requests in 10 minutes; the 60 bind first (GitLab.com rate limit docs,
//! 2026-11-23).

use serde_json::Value;

use super::Ctx;
use crate::catalog;
use crate::http::SourceError;
use crate::record::{Dataset, day, number, text};

const PAGE: usize = 100;
const OPENML_MIRROR: &str = "data/d/openml";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://gitlab.com/api/v4/projects")
        .query("search", catalog::terms(query).join(" "))
        .query("topic", "dataset")
        .query("order_by", "star_count")
        .query("sort", "desc")
        .query("per_page", PAGE)
        .query("simple", "true")
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let projects = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of projects"))?;
    let mirrored = |project: &&Value| {
        text(project, "/namespace/full_path").as_deref() == Some(OPENML_MIRROR)
    };
    Ok(projects
        .iter()
        .filter(|project| !mirrored(project))
        .filter_map(record)
        .take(limit)
        .collect())
}

pub(super) fn record(project: &Value) -> Option<Dataset> {
    let mut dataset = Dataset::new(
        &text(project, "/path_with_namespace")?,
        &text(project, "/web_url")?,
    )
    .describe(text(project, "/description"));
    dataset.publisher = text(project, "/namespace/full_path");
    dataset.updated = day(text(project, "/last_activity_at"));
    dataset.popularity = number(project, "/star_count");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("gitlab.json"), 10).unwrap();
        assert_eq!(
            hits[0],
            Dataset {
                title: "restcountries/restcountries".into(),
                url: "https://gitlab.com/restcountries/restcountries".into(),
                description: Some(
                    "REST Countries is a country data API: 250+ countries, \
                     80 fields each with 99.995% uptime."
                        .into()
                ),
                publisher: Some("restcountries".into()),
                doi: None,
                license: None,
                updated: Some("2026-06-20".into()),
                size_bytes: None,
                popularity: Some(156),
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_openml_mirror_group_is_dropped() {
        let body = fixture::json("gitlab.json");
        assert_eq!(body.as_array().unwrap().len(), 6);
        let hits = parse(&body, 10).unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "restcountries/restcountries",
                "reservoirdog/hist_thunderstorm_hail_central_europe",
            ]
        );
    }
}
