//! Figshare's public article search, item type 3 (dataset). Covers
//! figshare.com and the institutional portals on the same platform; each hit
//! links to the portal that holds it. The hits carry no description, license
//! or size; the full record has them, but one request per hit would break the
//! guideline of at most one request per second (Figshare, October 2026).
//! Results come newest first, because `order` defaults to `created_date`
//! descending and has no relevance value, and an article matches only when
//! every term appears somewhere in it (Figshare, October 2026). `updated` is
//! `modified_date`: `published_date` moves with each new version (Figshare,
//! October 2026). A page holds up to 1,000 hits, so one request covers
//! `--per-source` (Figshare, October 2026). A `search_for` shorter than 3
//! characters is refused with HTTP 422 (Figshare, October 2026), so such a
//! query gets no results instead of a failure.

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, text};

const MIN_QUERY_CHARS: usize = 3;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if query.chars().count() < MIN_QUERY_CHARS {
        return Ok(Vec::new());
    }
    let body = ctx
        .http
        .post("https://api.figshare.com/v2/articles/search")
        .json_body(json!({
            "search_for": query,
            "item_type": 3,
            "page_size": limit.clamp(1, 100),
        }))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let rows = body
        .as_array()
        .ok_or_else(|| SourceError::shape("expected a list of articles"))?;
    Ok(rows.iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let mut dataset =
        Dataset::new(&text(row, "/title")?, &text(row, "/url_public_html")?)
            .doi_from(text(row, "/doi"));
    dataset.updated =
        day(first_text(row, &["/modified_date", "/published_date"]));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{Services, fixture};

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("figshare.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "A Prospective Cooling Machine for the Svalbard \
                        Branch of Norwegian Atlantic Water in a Future \
                        Climate"
                    .into(),
                url: "https://figshare.com/articles/dataset/\
                      _b_A_Prospective_Cooling_Machine_for_the_Svalbard_\
                      Branch_of_Norwegian_Atlantic_Water_in_a_Future_\
                      Climate_b_/33059219"
                    .into(),
                description: None,
                publisher: None,
                doi: Some("10.6084/m9.figshare.33059219.v2".into()),
                license: None,
                updated: Some("2026-10-06".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[1].url,
            "https://frontiersin.figshare.com/articles/dataset/\
             Supplementary_file_1_Investigating_the_influence_of_\
             interannual_wind_forcing_on_the_South_Equatorial_Current_\
             and_spread_of_Indonesian_Throughflow_waters_docx/32753910"
        );
        assert_eq!(
            hits[1].doi.as_deref(),
            Some("10.3389/fmars.2026.1835248.s001")
        );
    }

    #[test]
    fn a_query_the_source_would_refuse_gets_no_results_and_no_request() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        for query in ["", "ml", "\u{dc}b"] {
            let hits = search(&services.ctx(false), query, 10);
            assert!(hits.unwrap().is_empty(), "{query:?}");
        }
    }
}
