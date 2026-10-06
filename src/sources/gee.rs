//! The Google Earth Engine public data catalog, read from its catalog page
//! (every dataset's id and title in one document) and searched locally. The
//! STAC mirror of the same catalog would take one request per dataset.

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

const MARK: &str = "data-label=\"toc-click-to-dataset-page ";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let page = ctx
        .http
        .get("https://developers.google.com/earth-engine/datasets/catalog")
        .slow()
        .text()?;
    let entries = parse(&page);
    if entries.is_empty() {
        return Err(SourceError::shape(
            "no dataset cards on the catalog page",
        ));
    }
    Ok(entries)
}

fn parse(page: &str) -> Vec<Dataset> {
    let mut seen = std::collections::HashSet::new();
    page.split(MARK)
        .skip(1)
        .filter_map(|card| {
            let (id, rest) = card.split_once('"')?;
            if !seen.insert(id.to_owned()) {
                return None;
            }
            let title = rest.split_once("data-text=\"")?.1.split_once('"')?.0;
            let mut dataset = Dataset::new(
                title,
                &format!(
                    "https://developers.google.com/earth-engine/datasets/catalog/{id}"
                ),
            )
            .describe(Some(id.replace('_', " ")));
            dataset.publisher = id.split('_').next().map(str::to_owned);
            dataset.valid()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn cards_yield_id_and_title_once() {
        let card = r#"<a href="/earth-engine/datasets/catalog/NASA_SRTM"
            data-label="toc-click-to-dataset-page NASA_SRTM">
            <h3 id="srtm" data-text="SRTM Digital Elevation" tabindex="-1">"#;
        let entries = parse(&format!("{card}{card}"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "SRTM Digital Elevation");
        assert!(entries[0].url.ends_with("/catalog/NASA_SRTM"));
    }

    #[test]
    fn records_map_from_a_recorded_list() {
        let entries = parse(&fixture::text("gee.html"));
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries[0],
            Dataset {
                title:
                    "2000 Greenland Mosaic - Greenland Ice Mapping Project \
                        (GIMP)"
                        .into(),
                url: "https://developers.google.com/earth-engine/datasets/\
                      catalog/OSU_GIMP_2000_IMAGERY_MOSAIC"
                    .into(),
                description: Some("OSU GIMP 2000 IMAGERY MOSAIC".into()),
                publisher: Some("OSU".into()),
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
