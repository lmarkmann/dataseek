//! TensorFlow Datasets' catalog, read from the overview page's links and
//! searched locally. Names only: the per-dataset pages carry the rest.

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

const LINK: &str = "href=\"/datasets/catalog/";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let page = ctx
        .http
        .get("https://www.tensorflow.org/datasets/catalog/overview")
        .slow()
        .text()?;
    let entries = parse(&page);
    if entries.is_empty() {
        return Err(SourceError::shape(
            "no catalog links on the overview page",
        ));
    }
    Ok(entries)
}

fn parse(page: &str) -> Vec<Dataset> {
    let mut seen = std::collections::HashSet::new();
    page.split(LINK)
        .skip(1)
        .filter_map(|link| {
            let name = link.split_once('"')?.0;
            let plain = name.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
            });
            if !plain || name == "overview" || !seen.insert(name.to_owned()) {
                return None;
            }
            Dataset::new(
                name,
                &format!("https://www.tensorflow.org/datasets/catalog/{name}"),
            )
            .describe(Some(name.replace('_', " ")))
            .valid()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn links_become_unique_entries() {
        let page = r#"<a href="/datasets/catalog/overview">x</a>
            <a href="/datasets/catalog/mnist">mnist</a>
            <a href="/datasets/catalog/mnist">again</a>
            <a href="/datasets/catalog/coco_captions">c</a>"#;
        let names: Vec<_> = parse(page).into_iter().map(|d| d.title).collect();
        assert_eq!(names, ["mnist", "coco_captions"]);
    }

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::text("tfds.html"));
        assert_eq!(hits.len(), 5);
        assert_eq!(
            hits[0],
            Dataset {
                title: "longt5".into(),
                url: "https://www.tensorflow.org/datasets/catalog/longt5"
                    .into(),
                description: Some("longt5".into()),
                publisher: None,
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(
            hits[4].description.as_deref(),
            Some("smartwatch gestures")
        );
    }
}
