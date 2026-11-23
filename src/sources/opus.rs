//! OPUS, the open collection of parallel corpora, read from the corpus list
//! of the OPUS API and searched locally. The list holds names and nothing
//! else: no description, language pairs, date, size or license. Matching is
//! therefore on the corpus name alone, so "Europarl" finds Europarl and
//! "parliament" does not.
//!
//! One response holds every corpus (1,215, 964 of them ELRC collections,
//! November 2026). Each name has a page at `/datasets/<name>`. robots.txt
//! disallows `/opusapi` for crawlers, against the documented API that
//! `opustools` calls; one catalog download a week is the whole load.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://opus.nlpl.eu/opusapi")
        .query("corpora", "True")
        .slow()
        .json()?;
    parse(&body)
}

fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/corpora", usize::MAX, corpus)
}

/// A corpus whose name is safe to put in the path of its page.
fn corpus(name: &Value) -> Option<Dataset> {
    let name = name.as_str().filter(|name| {
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    })?;
    Dataset::new(name, &format!("https://opus.nlpl.eu/datasets/{name}"))
        .valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::json("opus.json")).unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "ELRC-784-Coimisineir_Teanga_R",
                "ELRC-801-Coimisineir_Teanga_R",
                "OpenSubtitles",
                "Europarl"
            ]
        );
        assert_eq!(
            hits[3],
            Dataset {
                title: "Europarl".into(),
                url: "https://opus.nlpl.eu/datasets/Europarl".into(),
                description: None,
                publisher: None,
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_body_without_corpora_is_a_changed_shape() {
        let changed = serde_json::json!({"error": "unexpected"});
        assert!(matches!(parse(&changed), Err(SourceError::Shape(_))));
    }
}
