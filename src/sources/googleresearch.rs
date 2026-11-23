//! Google Research's dataset index, read from its listing pages and
//! searched locally. Each card holds a name, a one-paragraph description
//! and one outbound link: GitHub for most, then OpenSLR, Kaggle and Hugging
//! Face, and a few pages of their own. Those links let a card merge with
//! the same dataset from the source it points at.
//!
//! The index shows 15 cards a page (169 cards on 12 pages, November 2026).
//! A page past the end repeats the last one instead of failing, so the loop
//! stops at the first page that brings no new link, and after 30 pages at
//! most. robots.txt is `Allow: /`.

use std::collections::HashSet;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, clean};

const CARD: &str = "<a class=\" row-card\" href=\"";
const HEADING: &str = "row-card__heading js-gt-item-id\">";
const SUMMARY: &str = "row-card__subheading__item small-text\">";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries: Vec<Dataset> = Vec::new();
    let mut seen = HashSet::new();
    for page in 1..=30 {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        let html = ctx
            .http
            .get("https://research.google/resources/datasets/")
            .query("page", page)
            .slow()
            .text()?;
        if !html.contains("row-card-list") {
            return Err(SourceError::shape("no dataset list on the page"));
        }
        let cards = parse(&html);
        let before = seen.len();
        seen.extend(cards.iter().map(|card| card.url.clone()));
        if seen.len() == before {
            break;
        }
        entries.extend(cards);
    }
    Ok(entries)
}

fn parse(html: &str) -> Vec<Dataset> {
    html.split(CARD).skip(1).filter_map(card).collect()
}

fn card(html: &str) -> Option<Dataset> {
    let html = html.split_once("</a>").map_or(html, |(card, _)| card);
    let (link, rest) = html.split_once('"')?;
    Dataset::new(&inner(rest, HEADING)?, &clean(link))
        .describe(inner(rest, SUMMARY))
        .valid()
}

/// The text of the element that `marker` opens.
fn inner(html: &str, marker: &str) -> Option<String> {
    let (_, rest) = html.split_once(marker)?;
    let (text, _) = rest.split_once("</div>")?;
    Some(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::text("googleresearch.html"));
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Waxal NLP Dataset",
                "WikiProfile",
                "Cultural Familiarity Annotations"
            ]
        );
        assert_eq!(
            hits[2].url,
            "https://github.com/google-research-datasets/\
             cultural_familiarity_annotations/tree/main?tab=readme-ov-file"
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: "Waxal NLP Dataset".into(),
                url: "https://huggingface.co/datasets/google/WaxalNLP".into(),
                description: Some(
                    "The Waxal project provides datasets for both Automated \
                     Speech Recognition (ASR) and Text-to-Speech (TTS) for \
                     African languages. The goal of this dataset's creation \
                     and release is to facilitate research that improves \
                     the accuracy and fluency of speech and language \
                     technology for these languages."
                        .into()
                ),
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
}
