//! OpenSLR's resource index, read from its one HTML table and searched
//! locally. Each row holds the resource number, a name, a category (Speech,
//! Text, Audio, Misc, Software, Other) and a one-line summary; the category
//! and the summary become the description. Software rows are mirrors of
//! tools, not data, and are dropped. The index has no license, date or size:
//! those sit on each resource page.
//!
//! The list comes from the openslr.trmal.net mirror, which serves the same
//! table as www.openslr.org (155 rows, 33 KB, November 2026). Links point
//! at www.openslr.org, the address other catalogs cite. robots.txt on both
//! hosts disallows only `/cgi-bin/` and asks for a crawl delay of 5 seconds,
//! which one download a week respects.

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, clean};

const ROW: &str = "<tr><td>";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let page = ctx
        .http
        .get("https://openslr.trmal.net/resources.php")
        .slow()
        .text()?;
    if !page.contains("resourceTable") {
        return Err(SourceError::shape("no resource table on the page"));
    }
    Ok(parse(&page))
}

fn parse(page: &str) -> Vec<Dataset> {
    page.split(ROW).skip(1).filter_map(resource).collect()
}

/// One table row: the number link, then the name, category and summary.
fn resource(row: &str) -> Option<Dataset> {
    let row = row.split_once("</tr>").map_or(row, |(cells, _)| cells);
    let (number, cells) = row.split_once("href=\"/")?.1.split_once('/')?;
    let number: u32 = number.parse().ok()?;
    let mut cells = cells.split("<td>").skip(1).map(|cell| {
        clean(cell.split_once("</td>").map_or(cell, |(text, _)| text))
    });
    let name = cells.next()?;
    let category = cells.next()?;
    if category == "Software" {
        return None;
    }
    let summary = cells.next().unwrap_or_default();
    Dataset::new(&name, &format!("https://www.openslr.org/{number}/"))
        .describe(Some(format!("{category} resource. {summary}")))
        .valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::text("openslr.html"));
        let links: Vec<&str> = hits.iter().map(|h| h.url.as_str()).collect();
        assert_eq!(
            links,
            ["https://www.openslr.org/1/", "https://www.openslr.org/5/"],
            "the three Software rows are dropped"
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: "Yesno".into(),
                url: "https://www.openslr.org/1/".into(),
                description: Some(
                    "Speech resource. Sixty recordings of one individual \
                     saying yes or no in Hebrew; each recording is eight \
                     words long."
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
