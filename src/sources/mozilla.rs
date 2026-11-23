//! Mozilla Data Collective, the speech and language data platform where
//! Common Voice releases ship since 2025-10-31, searched through its public
//! catalog API.
//!
//! `GET /api/datasets` takes `q` and answers `items` in relevance order. The
//! match is loose: a query of several words, or of no real word, still
//! answers up to 100 datasets that share a part of it. `limit` goes up to
//! 100; 101 answers 400. The API is marked beta. The catalog search needs no
//! key; a download needs a key and a terms agreement in the web interface,
//! so dataseek only links to the dataset page. The headers allow 120
//! requests a window, and the window reset within a minute (live API,
//! 2026-11-23).
//!
//! Some datasets are sold. A paid dataset is never shown as one to download:
//! the request asks for `pricing=free`, and a row with `pricing.isPaid` set
//! is dropped all the same.
//!
//! A row carries an SPDX `licenseAbbreviation`, a byte size, a creation
//! date and a comma-separated `format`, but no DOI and no update date.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, number, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://mozilladatacollective.com/api/datasets")
        .query("q", query)
        .query("pricing", "free")
        .query("limit", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/items", limit, record)
}

fn record(row: &Value) -> Option<Dataset> {
    if row.pointer("/pricing/isPaid").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let mut dataset =
        Dataset::new(&text(row, "/name")?, &text(row, "/datasetUrl")?)
            .describe(first_text(
                row,
                &["/longDescription", "/shortDescription"],
            ));
    dataset.publisher = text(row, "/organization/name");
    dataset.license = first_text(row, &["/licenseAbbreviation", "/license"]);
    dataset.updated = day(text(row, "/createdAt"));
    dataset.size_bytes = number(row, "/sizeBytes");
    dataset.formats = text(row, "/format").map_or_default(|formats| {
        formats.split(',').map(|f| f.trim().to_owned()).collect()
    });
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("mozilla.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Common Voice Scripted Speech 27.0 - German".into(),
                url: "https://mozilladatacollective.com/datasets/\
                      cmu5ktqhe00pgmh07p1xamabd"
                    .into(),
                description: Some(
                    "A collection of read speech recordings in German \
                     (Deutsch)."
                        .into()
                ),
                publisher: Some("Mozilla Common Voice".into()),
                doi: None,
                license: Some("CC0-1.0".into()),
                updated: Some("2026-09-17".into()),
                size_bytes: Some(37_382_527_376),
                popularity: None,
                formats: vec!["MP3".into()],
                aliases: vec![],
            }
        );
        assert_eq!(hits[2].title, "CoVoST 2 German-English");
        assert_eq!(hits[2].formats, ["MP3", "TSV"]);
        assert_eq!(hits[2].license.as_deref(), Some("CC-BY-NC-4.0"));
    }

    #[test]
    fn a_paid_dataset_is_never_listed() {
        let body = json!({"items": [
            {
                "name": "Paid corpus",
                "datasetUrl": "https://mozilladatacollective.com/datasets/a",
                "pricing": {"isPaid": true},
            },
            {
                "name": "Unpriced corpus",
                "datasetUrl": "https://mozilladatacollective.com/datasets/b",
            },
            {
                "name": "Free corpus",
                "datasetUrl": "https://mozilladatacollective.com/datasets/c",
                "pricing": {"isPaid": false},
            },
        ]});
        let hits = parse(&body, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Free corpus");
    }
}
