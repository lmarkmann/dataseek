//! The Registry of Open Data on AWS, read from the registry's index page,
//! which lists every entry with its name and one-paragraph description.
//! Searched locally. The source YAML lives on GitHub but would cost one
//! request per entry.

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, clean};

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let page = ctx.http.get("https://registry.opendata.aws/").slow().text()?;
    let entries = parse(&page);
    if entries.is_empty() {
        return Err(SourceError::shape(
            "no dataset blocks on the registry page",
        ));
    }
    Ok(entries)
}

fn parse(page: &str) -> Vec<Dataset> {
    page.split("<div id=\"")
        .skip(1)
        .filter_map(|block| {
            let (id, rest) = block.split_once('"')?;
            if !rest.trim_start().starts_with("class=\"dataset\"") {
                return None;
            }
            let title =
                between(rest, "<h3><a href=\"", "</a>")?.split_once('>')?.1;
            let description = rest
                .split("<p>")
                .skip(1)
                .filter_map(|p| p.split_once("</p>").map(|(inner, _)| inner))
                .map(str::trim)
                .find(|p| {
                    let tags = p.starts_with("<span");
                    let link = p.starts_with("<a ") && p.ends_with("</a>");
                    !tags && !link && !clean(p).is_empty()
                });
            Dataset::new(
                title,
                &format!("https://registry.opendata.aws/{id}/"),
            )
            .describe(description.map(str::to_owned))
            .valid()
        })
        .collect()
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)?.saturating_add(open.len());
    let rest = text.get(start..)?;
    rest.get(..rest.find(close)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn dataset_blocks_parse_and_others_are_ignored() {
        let page = r#"<div id="nav" class="x"></div>
        <div id="commoncrawl" class="dataset">
          <h3><a href="/commoncrawl/">Common Crawl</a></h3>
          <p><span class="label">web</span></p>
          <p>A corpus of web crawl data.</p>
          <p><a href="/commoncrawl/">Details &rarr;</a></p>
        </div>"#;
        let entries = parse(page);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Common Crawl");
        assert_eq!(
            entries[0].url,
            "https://registry.opendata.aws/commoncrawl/"
        );
        assert_eq!(
            entries[0].description.as_deref(),
            Some("A corpus of web crawl data.")
        );
    }

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::text("aws.html"));
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Common Crawl".into(),
                url: "https://registry.opendata.aws/commoncrawl/".into(),
                description: Some(
                    "A corpus of web crawl data composed of over 300 billion \
                     web pages."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        let described =
            |i: usize| hits[i].description.clone().unwrap_or_default();
        assert!(
            described(1).starts_with("Disk images, memory dumps"),
            "{}",
            described(1)
        );
        assert!(
            described(2).starts_with("The Gridded Altimeter Fields"),
            "{}",
            described(2)
        );
    }
}
