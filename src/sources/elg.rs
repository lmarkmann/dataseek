//! The European Language Grid catalogue, which aggregates language
//! resources from ELRC-SHARE, the ELRA catalogue, CLARIN.SI, LINDAT,
//! CLARIN-PL, Zenodo and Hugging Face, searched through its registry API.
//!
//! `GET /catalogue_backend/api/registry/search/` with
//! `entity_type__term=LanguageResource` takes the words in `search`, as the
//! ELG Python SDK sends them, and answers `results` in relevance order.
//! `keyword` is not a text search: it matches one keyword value exactly, so
//! "parliament speech" finds nothing there and 2,167 resources through
//! `search`. A page holds 20 results and no parameter changes that; `page`
//! walks on while `next` is set (live API and SDK docs, 2026-11-23). No rate
//! limit is published.
//!
//! The catalogue lists tools, services and models beside data: of 16,207
//! language resources, 3,971 are a Tool/Service and 433 a Model. Those are
//! dropped, so one more page than the limit needs is asked for. A repeated
//! `resource_type__term` is read as AND, so the filter cannot run on the
//! server.
//!
//! A search hit carries licence names, a short licence id and the update
//! date, but no DOI and no link to the source it was harvested from: those
//! sit in the metadata record, one more request each. The link is the
//! catalogue page, whose path names the resource type. `source` names the
//! repository the record came from, which is the publisher.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, number, text};

const PAGE: usize = 20;

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let last = limit.div_ceil(PAGE).saturating_add(1);
    let first = fetch(ctx, query, 1)?;
    super::paginate_next(ctx, limit, last, first, parse, |page| {
        fetch(ctx, query, page)
    })
}

fn fetch(
    ctx: &Ctx<'_>,
    query: &str,
    page: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get(
            "https://live.european-language-grid.eu/catalogue_backend/api/\
             registry/search/",
        )
        .query("entity_type__term", "LanguageResource")
        .query("search", query)
        .query("page", page)
        .json()
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/results", limit, record)
}

fn record(row: &Value) -> Option<Dataset> {
    let kind = catalogue_path(&text(row, "/resource_type")?)?;
    let id = text(row, "/id")?;
    let mut dataset = Dataset::new(
        &text(row, "/resource_name")?,
        &format!(
            "https://live.european-language-grid.eu/catalogue/{kind}/{id}"
        ),
    )
    .describe(text(row, "/description"));
    dataset.publisher = text(row, "/source");
    dataset.license =
        first_text(row, &["/licence_short_name/0", "/licences/0"]);
    dataset.updated = day(text(row, "/last_date_updated"));
    dataset.popularity = number(row, "/views");
    dataset.valid()
}

/// The catalogue page's path segment for a resource type that holds data;
/// `None` for tools, services and models.
fn catalogue_path(resource_type: &str) -> Option<&'static str> {
    match resource_type {
        "Corpus" => Some("corpus"),
        "Lexical/Conceptual resource" => Some("lcr"),
        "Grammar" | "Uncategorized Language Description" => Some("ld"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("elg.json"), 10).unwrap();
        assert_eq!(hits.len(), 3, "the Tool/Service row is dropped");
        assert_eq!(
            hits[0],
            Dataset {
                title: "EU Parliament Speech corpus".into(),
                url: "https://live.european-language-grid.eu/catalogue/\
                      corpus/8684"
                    .into(),
                description: Some(
                    "A collection of 1040 EU parliament speeches with \
                     transcription and annotations. Includes original \
                     speeches and PL/EN translations."
                        .into()
                ),
                publisher: Some("CLARIN-PL".into()),
                doi: None,
                license: Some("CC-BY-4.0".into()),
                updated: Some("2021-09-30".into()),
                size_bytes: None,
                popularity: Some(86),
                formats: vec![],
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[2].url,
            "https://live.european-language-grid.eu/catalogue/lcr/15133"
        );
        assert_eq!(hits[2].title, "Austrian Parliament Abkürzungsverzeichnis");
        assert_eq!(hits[2].license, None);
    }

    #[test]
    fn a_page_without_next_ends_the_walk() {
        let body = fixture::json("elg.json");
        assert!(super::super::has_next(&body));
        assert!(!super::super::has_next(&serde_json::json!({"next": null})));
    }
}
