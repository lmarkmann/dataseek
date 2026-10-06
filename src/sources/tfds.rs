//! TensorFlow Datasets' catalog, read from the overview page and searched
//! locally. The page lists every dataset under each category it belongs to
//! (446 datasets, October 2026, tensorflow.org), and those category names
//! become the description. The real descriptions sit on one page per dataset,
//! which is too many requests for a catalog download. robots.txt on
//! tensorflow.org disallows nothing (October 2026).

use std::collections::HashMap;

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

const LINK: &str = "href=\"/datasets/catalog/";
const HEADING: &str = "data-text=\"";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let page = ctx
        .http
        .get("https://www.tensorflow.org/datasets/catalog/overview")
        .slow()
        .text()?;
    let entries = parse(&page);
    if entries.is_empty() {
        return Err(SourceError::shape(
            "no All Datasets listing on the overview page",
        ));
    }
    Ok(entries)
}

fn parse(page: &str) -> Vec<Dataset> {
    let Some((_, listing)) = page.split_once("\"all_datasets\"") else {
        return Vec::new();
    };
    let mut sections: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut order = Vec::new();
    for section in listing.split(HEADING).skip(1) {
        let Some((category, rest)) = section.split_once('"') else {
            continue;
        };
        let links = rest.split_once("</ul>").map_or(rest, |(list, _)| list);
        for link in links.split(LINK).skip(1) {
            let Some((name, _)) = link.split_once('"') else {
                continue;
            };
            let plain = name.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
            });
            if !plain || name == "overview" {
                continue;
            }
            let categories = sections.entry(name).or_insert_with(|| {
                order.push(name);
                Vec::new()
            });
            if category != "Uncategorized" {
                categories.push(category);
            }
        }
    }
    order
        .into_iter()
        .filter_map(|name| {
            Dataset::new(
                name,
                &format!("https://www.tensorflow.org/datasets/catalog/{name}"),
            )
            .describe(Some(sections.get(name)?.join(", ")))
            .valid()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn a_dataset_keeps_every_category_it_is_listed_under() {
        let page = r#"<a href="/datasets/catalog/nav_only">nav</a>
            <h2 id="all_datasets" data-text="All Datasets">All</h2>
            <h3 id="a" data-text="Audio"></h3><ul>
            <li><a href="/datasets/catalog/overview">x</a></li>
            <li><a href="/datasets/catalog/mnist">mnist</a></li>
            <li><a href="/datasets/catalog/coco_captions">c</a></li></ul>
            <h3 id="u" data-text="Uncategorized"></h3><ul>
            <li><a href="/datasets/catalog/mnist">again</a></li>
            <li><a href="/datasets/catalog/qm9">q</a></li></ul>
            <a href="/datasets/catalog/footer_only">footer</a>"#;
        let found: Vec<_> = parse(page)
            .into_iter()
            .map(|d| (d.title, d.description))
            .collect();
        assert_eq!(
            found,
            [
                ("mnist".to_owned(), Some("Audio".to_owned())),
                ("coco_captions".to_owned(), Some("Audio".to_owned())),
                ("qm9".to_owned(), None),
            ]
        );
    }

    #[test]
    fn a_page_without_the_listing_has_no_entries() {
        let page = r#"<a href="/datasets/catalog/mnist">mnist</a>"#;
        assert_eq!(parse(page).len(), 0);
    }

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::text("tfds.html"));
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "longt5",
                "xtreme",
                "aflw2k3d",
                "smallnorb",
                "accentdb",
                "yes_no",
                "mnist",
                "qm9",
                "gref",
                "wit"
            ]
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: "longt5".into(),
                url: "https://www.tensorflow.org/datasets/catalog/longt5"
                    .into(),
                description: Some("Dataset Collections".into()),
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
            hits[3].description.as_deref(),
            Some("3d, Image, Image classification")
        );
        assert_eq!(
            hits[4].description.as_deref(),
            Some("Audio, Speech recognition")
        );
        assert_eq!(hits[7].description, None);
    }
}
