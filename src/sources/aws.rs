//! The Registry of Open Data on AWS, read from the registry's index page,
//! which lists every entry with its name and one-paragraph description.
//! Searched locally. The source YAML lives on GitHub but would cost one
//! request per entry.

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

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
                .find(|p| !p.contains("<span") && !p.contains("Details"));
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
}
