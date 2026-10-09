//! Kaggle dataset search through the official API. The user's own token is
//! sent when one is found (see `credentials.rs`); the endpoint answered the
//! same without one, or with an invalid one (Kaggle, October 2026). A page
//! holds 20 datasets and ignores a page size, so a larger limit reads the
//! next pages with `page`, which the API documents, in the default hottest
//! order (Kaggle, October 2026). Results are never cached to disk, because
//! Kaggle's terms forbid storing a significant portion of content (Kaggle,
//! June 2025).

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::{Dataset, day, number, text};

const PAGE_SIZE: usize = 20;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    paged(limit, |page| {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        let mut call = ctx
            .http
            .get("https://www.kaggle.com/api/v1/datasets/list")
            .query("search", query)
            .query("page", page);
        if let Some(secret) = ctx.creds.get(Key::Kaggle) {
            call = call.header("Authorization", secret.authorization());
        }
        call.json()
    })
}

/// Pages 1, 2, ... until `limit` datasets are read or a page comes back
/// short, which means Kaggle has no more. A failure on the first page is the
/// source's failure; one later keeps the pages already read.
fn paged(
    limit: usize,
    mut fetch: impl FnMut(usize) -> Result<Value, SourceError>,
) -> Result<Vec<Dataset>, SourceError> {
    let mut found = Vec::new();
    for page in 1..=limit.div_ceil(PAGE_SIZE) {
        let wanted = limit.saturating_sub(found.len());
        let read = fetch(page)
            .and_then(|body| Ok((parse(&body, wanted)?, rows_in(&body))));
        let (datasets, rows) = match read {
            Ok(read) => read,
            Err(error) if page == 1 => return Err(error),
            Err(_) => break,
        };
        found.extend(datasets);
        if rows < PAGE_SIZE {
            break;
        }
    }
    Ok(found)
}

fn rows_in(body: &Value) -> usize {
    body.as_array().map_or(0, Vec::len)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of datasets"))?;
    Ok(rows.iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let reference = text(row, "/ref")?;
    let url = text(row, "/url").unwrap_or_else(|| {
        format!("https://www.kaggle.com/datasets/{reference}")
    });
    let mut dataset = Dataset::new(&text(row, "/title")?, &url)
        .describe(text(row, "/subtitle"));
    dataset.publisher = text(row, "/ownerName");
    dataset.license =
        text(row, "/licenseName").filter(|name| name != "Unknown");
    dataset.updated = day(text(row, "/lastUpdated"));
    dataset.size_bytes = number(row, "/totalBytes");
    dataset.popularity = number(row, "/downloadCount");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    /// A full-size page of the recorded first row, each with its own link.
    fn page_of(rows: usize, page: usize) -> Value {
        let recorded = fixture::json("kaggle.json");
        let rows = (0..rows)
            .map(|n| {
                let mut row = recorded[0].clone();
                row["url"] = format!(
                    "https://www.kaggle.com/datasets/owner/page{page}-{n}"
                )
                .into();
                row
            })
            .collect();
        Value::Array(rows)
    }

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("kaggle.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Climate change Indicators".into(),
                url: "https://www.kaggle.com/datasets/\
                      tarunrm09/climate-change-indicators"
                    .into(),
                description: Some(
                    "Climate change Indicators suggesting the surface \
                     temperature change annually"
                        .into()
                ),
                publisher: Some("Tarun Mugesh".into()),
                doi: None,
                license: Some("CC0: Public Domain".into()),
                updated: Some("2024-02-22".into()),
                size_bytes: Some(34_794),
                popularity: Some(20_272),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn an_unknown_license_is_no_license() {
        let mut row = fixture::json("kaggle.json")[0].clone();
        row["licenseName"] = "Unknown".into();
        let hits = parse(&Value::Array(vec![row]), 10).unwrap();
        assert_eq!(hits[0].license, None);
    }

    #[test]
    fn a_larger_limit_reads_the_next_pages_in_order() {
        let mut asked = Vec::new();
        let hits = paged(50, |page| {
            asked.push(page);
            Ok(page_of(PAGE_SIZE, page))
        })
        .unwrap();
        assert_eq!(asked, [1, 2, 3]);
        assert_eq!(hits.len(), 50);
        assert_eq!(
            hits[0].url,
            "https://www.kaggle.com/datasets/owner/page1-0"
        );
        assert_eq!(
            hits[20].url,
            "https://www.kaggle.com/datasets/owner/page2-0"
        );
        assert_eq!(
            hits[49].url,
            "https://www.kaggle.com/datasets/owner/page3-9"
        );
    }

    #[test]
    fn a_limit_within_one_page_asks_once() {
        let mut asked = Vec::new();
        let hits = paged(5, |page| {
            asked.push(page);
            Ok(page_of(PAGE_SIZE, page))
        })
        .unwrap();
        assert_eq!((asked, hits.len()), (vec![1], 5));
    }

    #[test]
    fn a_short_page_ends_the_paging() {
        let mut asked = Vec::new();
        let hits = paged(100, |page| {
            asked.push(page);
            Ok(page_of(if page == 1 { PAGE_SIZE } else { 7 }, page))
        })
        .unwrap();
        assert_eq!((asked, hits.len()), (vec![1, 2], 27));
    }

    #[test]
    fn a_failure_after_the_first_page_keeps_what_was_read() {
        let hits = paged(100, |page| match page {
            1 => Ok(page_of(PAGE_SIZE, 1)),
            _ => Err(SourceError::RateLimited),
        })
        .unwrap();
        assert_eq!(hits.len(), PAGE_SIZE);
    }

    #[test]
    fn a_failure_on_the_first_page_is_the_sources_failure() {
        let outcome = paged(100, |_| Err(SourceError::RateLimited));
        assert!(
            matches!(outcome, Err(SourceError::RateLimited)),
            "{outcome:?}"
        );
    }
}
