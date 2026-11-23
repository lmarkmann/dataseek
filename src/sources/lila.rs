//! LILA BC (Labeled Information Library of Alexandria: Biology and
//! Conservation), read from the WordPress posts API of lila.science and
//! searched locally. Each dataset is one post: camera-trap, fish, drone and
//! aerial imagery, most with species labels or bounding boxes.
//!
//! One page of 100 posts holds every post (61, of which 59 are datasets,
//! November 2026); the loop asks for the next page only while a page is
//! full. Posts outside `/datasets/` are site pages and are dropped. The
//! excerpt is the description, without the "Overview" heading that opens
//! nearly every one; `modified` is the update date. robots.txt disallows
//! nothing.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, clean, day, text};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let posts = super::wordpress_rows(
        ctx,
        "https://lila.science/wp-json/wp/v2/posts",
        "title,link,excerpt,modified",
    )?;
    Ok(parse(&posts))
}

fn parse(posts: &[Value]) -> Vec<Dataset> {
    posts
        .iter()
        .filter_map(|post| {
            let link = text(post, "/link")?;
            if !link.contains("/datasets/") {
                return None;
            }
            let excerpt = clean(&text(post, "/excerpt/rendered")?);
            let excerpt =
                excerpt.strip_prefix("Overview ").unwrap_or(&excerpt);
            let mut dataset =
                Dataset::new(&text(post, "/title/rendered")?, &link)
                    .describe(Some(excerpt.to_owned()));
            dataset.updated = day(text(post, "/modified"));
            dataset.valid()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let body = fixture::json("lila.json");
        let hits = parse(body.as_array().unwrap());
        assert_eq!(hits.len(), 2, "the site page is dropped");
        assert_eq!(hits[1].title, "Duck Pictures in Wetlands");
        assert_eq!(
            hits[0],
            Dataset {
                title: "Indiana Small Animals".into(),
                url: "https://lila.science/datasets/indiana-small-animals/"
                    .into(),
                description: Some(
                    "This dataset contains 1.6 million images containing \
                     302,582 unique detections (sequences) of small-bodied \
                     wildlife. Images were collected from 102 sites \
                     throughout Indiana, USA using the AHDriFT camera trap \
                     system (a drift fence between inverted buckets \
                     containing downward-facing camera traps). Labels are \
                     provided for..."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: Some("2026-11-18".into()),
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
    }
}
