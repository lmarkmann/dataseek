//! PANGAEA's search service. Its JSON carries each hit's citation as an HTML
//! fragment, `<strong>Author (year):</strong> Title</a>`, which is where the
//! title and authors come from.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, clean, doi, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://www.pangaea.de/advanced/search.php")
        .query("q", query)
        .query("count", limit.clamp(1, 500))
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(body, "/results").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let found = text(row, "/URI").as_deref().and_then(doi)?;
    let html = text(row, "/html")?;
    let (authors, rest) =
        html.split_once("<strong>")?.1.split_once("</strong>")?;
    let title = rest.split_once("</a>")?.0;
    let abstract_text = html
        .split_once("Abstract:")
        .and_then(|(_, tail)| tail.split_once("<td class=\"content\">"))
        .and_then(|(_, tail)| tail.split_once("</td>"))
        .map(|(inner, _)| inner.to_owned());
    let mut dataset = Dataset::new(title, &format!("https://doi.org/{found}"))
        .describe(abstract_text);
    dataset.doi = Some(found);
    dataset.publisher = Some(clean(authors).trim_end_matches(':').to_owned());
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("pangaea.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Sea ice thickness and sea ice area transport in the \
                        Laptev Sea"
                    .into(),
                url: "https://doi.org/10.1594/pangaea.880357".into(),
                description: Some(
                    "Recent studies based on satellite observations have \
                     shown that there is a high statistical connection \
                     between the late winter (Feb-May) sea ice export out the \
                     Laptev Sea, and the ice coverage in the following summer."
                        .into()
                ),
                publisher: Some("Krumpen, T (2017)".into()),
                doi: Some("10.1594/pangaea.880357".into()),
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
