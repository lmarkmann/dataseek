//! The CLARIN Virtual Language Observatory, the metadata index of the
//! CLARIN language resource centres, searched through the VLO API.
//!
//! `GET /api/records` takes a Solr `q`, `fq` facet filters, `from` and
//! `size`, and answers `records` in relevance order. `fq=resourceClass:corpus`
//! is always sent: without it, single recording sessions of a speech
//! archive fill the list. Quotes, parentheses and a trailing `AND` do not
//! fail the search; a word with a colon is read as a field query (VLO API
//! docs and live API, 2026-11-23). `robots.txt` asks for a `Crawl-delay`
//! of 5 seconds and leaves `/api` open; one search is one request.
//!
//! A search record carries `name`, `description`, `license` and
//! `availability`, but no date, size or landing page: `landingPages` is
//! empty in every search record, and a single record's detail fills more
//! fields at one more request each. The self link is the persistent
//! identifier of the CMDI record, mostly a Handle with `@format=cmdi` or a
//! Finnish URN:NBN. Each resolves to the centre's landing page, which is
//! the link; a record without one links to its VLO page. Most centres mint
//! Handles, not DOIs, so a record carries no DOI.
//!
//! Each description starts with its language, as in `{code:eng}`: the
//! English one is taken, else the first, without that prefix. The harvest
//! path names the centre, which is the publisher. `availability` (PUB,
//! ACA, RES) says who may download; a record of every level is listed.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, from_markdown, items, text};

const VLO: &str = "https://vlo.clarin.eu";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!("{VLO}/api/records"))
        .query("q", query)
        .query("fq", "resourceClass:corpus")
        .query("size", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/records", limit, record)
}

fn record(row: &Value) -> Option<Dataset> {
    let name = text(row, "/fields/name/0")?;
    let link = text(row, "/selflink")
        .and_then(|selflink| resolver(&selflink))
        .or_else(|| text(row, "/id").map(|id| format!("{VLO}/record/{id}")))?;
    let mut dataset = Dataset::new(&name, &link)
        .describe(description(row).map(|d| from_markdown(&d)));
    dataset.publisher =
        text(row, "/sourcePath").and_then(|path| centre(&path));
    dataset.license = text(row, "/fields/license/0");
    dataset.valid()
}

/// The resolver link of a Handle or Finnish URN:NBN self link, without the
/// CMDI format suffix.
fn resolver(selflink: &str) -> Option<String> {
    let handle = selflink
        .strip_prefix("hdl:")
        .or_else(|| selflink.split_once("hdl.handle.net/").map(|(_, h)| h));
    if let Some(handle) = handle {
        let handle = handle.split('@').next()?;
        return Some(format!("https://hdl.handle.net/{handle}"));
    }
    selflink
        .starts_with("urn:nbn:fi:")
        .then(|| format!("https://urn.fi/{selflink}"))
}

/// The English description, else the first, without its language prefix.
fn description(row: &Value) -> Option<String> {
    let texts: Vec<&str> = items(row, "/fields/description")
        .iter()
        .filter_map(Value::as_str)
        .filter(|d| !d.trim().is_empty())
        .collect();
    let chosen = texts
        .iter()
        .find(|d| d.starts_with("{code:eng}"))
        .or_else(|| texts.first())?;
    let plain = match chosen.strip_prefix("{code:") {
        Some(rest) => rest.split_once('}').map_or(rest, |(_, d)| d),
        None => chosen,
    };
    Some(plain.to_owned())
}

/// The centre's name from the harvest directory the record came from.
fn centre(source_path: &str) -> Option<String> {
    let directory = source_path.rsplit('/').nth(1)?;
    let name = directory.replace('_', " ");
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("clarin.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "EU Parliament Speech corpus".into(),
                url: "https://vlo.clarin.eu/record/https_58__47__47_\
                      clarin-pl.eu_47_server_47_cmdi_47_oai-metadata_63_\
                      metadataPrefix_61_cmdi_38_handle_61_11321_47_821"
                    .into(),
                description: Some(
                    "A collection of 1040 EU parliament speeches with \
                     transcription and annotations. Includes original \
                     speeches and PL/EN translations."
                        .into()
                ),
                publisher: Some("CLARIN PL Repository".into()),
                doi: None,
                license: Some(
                    "https://creativecommons.org/licenses/by/4.0/".into()
                ),
                updated: None,
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
        assert_eq!(hits[1].url, "https://hdl.handle.net/20.500.12537/197");
        assert_eq!(hits[1].publisher.as_deref(), Some("CLARIN IS"));
        assert_eq!(hits[2].url, "https://urn.fi/urn:nbn:fi:lb-2022052002");
        assert!(
            hits[2]
                .description
                .as_deref()
                .unwrap()
                .starts_with("Aalto Finnish Parliament ASR Corpus"),
            "the English description, without its heading mark"
        );
    }

    #[test]
    fn a_self_link_resolves_to_its_handle_or_urn() {
        for (selflink, link) in [
            (
                "hdl:11495/D9D1-ED78-58C4-1",
                "https://hdl.handle.net/11495/D9D1-ED78-58C4-1",
            ),
            (
                "http://hdl.handle.net/11500/ATHENA-0000-0000-5D65-7@format=cmdi",
                "https://hdl.handle.net/11500/ATHENA-0000-0000-5D65-7",
            ),
            (
                "urn:nbn:fi:lb-2017020202",
                "https://urn.fi/urn:nbn:fi:lb-2017020202",
            ),
        ] {
            assert_eq!(
                resolver(selflink).as_deref(),
                Some(link),
                "{selflink}"
            );
        }
        assert_eq!(resolver("https://clarin-pl.eu/server/cmdi/x"), None);
    }

    #[test]
    fn a_description_without_english_falls_back_to_the_first() {
        let row = json!({"fields": {"description": [
            " ", "{code:fin}Tämä aineisto", "{code:swe}Detta material",
        ]}});
        assert_eq!(description(&row).as_deref(), Some("Tämä aineisto"));
    }
}
