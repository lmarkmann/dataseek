//! Roboflow Universe, computer-vision datasets. Requires the user's Roboflow
//! API key, which travels in an `Authorization: Bearer` header. The endpoint
//! also takes it as `api_key`, and a placeholder token is judged the same way
//! in both places, but that would put the key in the URL (Roboflow API,
//! October 2026).
//!
//! A response can only be recorded with a key, and none was used, so the
//! layout is what Roboflow's own clients read: the Python SDK and CLI take a
//! `results` list whose hits carry `name`, `type`, `images` and `url`, and
//! the docs' example hit adds `thumbnail` and `annotationThumbnail` (Roboflow
//! docs and roboflow-python, October 2026). The list is looked for under the
//! keys the SDK might read, and anything else is a shape error.
//!
//! The docs list `q` and `page`, counted from 1, and no page size; the SDK's
//! `limit` defaults to 12 and its CLI says the API may ignore it. So `page`
//! alone walks the results, until the limit is met or a page brings nothing
//! new, in at most ten requests. A page that fails after the first keeps what
//! the earlier ones brought. No rate limit is published; one over it answers
//! 429 (Roboflow docs and roboflow-python, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, first_text, number};

const MAX_PAGES: usize = 10;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let secret =
        ctx.creds.get(Key::Roboflow).ok_or(SourceError::Unauthorized(401))?;
    collect_pages(limit, |page| {
        ctx.http
            .get("https://api.roboflow.com/universe/search")
            .query("q", query)
            .query("page", page)
            .header("Authorization", secret.authorization())
            .json()
    })
}

fn collect_pages(
    limit: usize,
    mut fetch: impl FnMut(usize) -> Result<Value, SourceError>,
) -> Result<Vec<Dataset>, SourceError> {
    let mut found: Vec<Dataset> = Vec::new();
    for page in 1..=MAX_PAGES {
        let hits = match fetch(page).and_then(|body| parse(&body, limit)) {
            Ok(hits) => hits,
            Err(_) if !found.is_empty() => break,
            Err(error) => return Err(error),
        };
        let held = found.len();
        for hit in hits {
            if !found.iter().any(|seen| seen.url == hit.url) {
                found.push(hit);
            }
        }
        if found.len() >= limit || found.len() == held {
            break;
        }
    }
    found.truncate(limit);
    Ok(found)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    fn page_of(names: &[&str]) -> Value {
        let hits: Vec<Value> = names
            .iter()
            .map(|n| json!({"name": n, "url": format!("https://universe.roboflow.com/w/{n}")}))
            .collect();
        json!({ "results": hits })
    }

    // The example hit is from
    // https://docs.roboflow.com/datasets/universe/universe/what-is-roboflow-universe
    // and the `results` list around it from the SDK page
    // https://docs.roboflow.com/datasets/universe/universe/universe-search
    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("roboflow.json"), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Cars".into(),
                url: "https://universe.roboflow.com/growth-plan/cars-8q9vz"
                    .into(),
                description: None,
                publisher: None,
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
    fn pages_are_read_until_the_limit_is_met() {
        let pages = [page_of(&["a", "b"]), page_of(&["c", "d"])];
        let mut asked = Vec::new();
        let hits = collect_pages(3, |page| {
            asked.push(page);
            Ok(pages[page - 1].clone())
        })
        .unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(titles, ["a", "b", "c"]);
        assert_eq!(asked, [1, 2]);
    }

    #[test]
    fn paging_stops_when_a_page_adds_nothing_new() {
        let mut asked = 0;
        let hits = collect_pages(50, |page| {
            asked += 1;
            Ok(if page == 1 { page_of(&["a", "b"]) } else { page_of(&[]) })
        })
        .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(asked, 2);

        let mut asked = 0;
        let hits = collect_pages(50, |_| {
            asked += 1;
            Ok(page_of(&["a", "b"]))
        })
        .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(asked, 2);
    }

    #[test]
    fn a_page_that_fails_keeps_the_earlier_ones() {
        let hits = collect_pages(50, |page| match page {
            1 => Ok(page_of(&["a", "b"])),
            _ => Err(SourceError::RateLimited),
        })
        .unwrap();
        assert_eq!(hits.len(), 2);

        let first = collect_pages(50, |_| Err(SourceError::RateLimited));
        assert!(matches!(first, Err(SourceError::RateLimited)));
    }

    #[test]
    fn paging_is_capped() {
        let mut asked = 0;
        let hits = collect_pages(100, |page| {
            asked += 1;
            Ok(page_of(&[&format!("n{page}")]))
        })
        .unwrap();
        assert_eq!(asked, MAX_PAGES);
        assert_eq!(hits.len(), MAX_PAGES);
    }
}
