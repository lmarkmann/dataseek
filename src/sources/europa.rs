//! data.europa.eu, the EU portal that harvests DCAT-AP from EU institutions
//! and the national and regional portals of member states. One adapter for
//! European government open data; fields arrive as language maps.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, localized, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://data.europa.eu/api/hub/search/search")
        .query("q", query)
        .query("filter", "dataset")
        .query("limit", limit)
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/result/results").is_none() {
        return Err(SourceError::shape("no result.results"));
    }
    Ok(items(body, "/result/results")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let title = localized(row.get("title"))?;
    let mut dataset = Dataset::new(
        &title,
        &format!("https://data.europa.eu/data/datasets/{id}"),
    )
    .describe(localized(row.get("description")));
    dataset.publisher = row
        .pointer("/publisher/name")
        .and_then(|v| localized(Some(v)))
        .or_else(|| localized(row.pointer("/catalog/title")));
    dataset.license = items(row, "/distributions").iter().find_map(|d| {
        localized(d.pointer("/license/label"))
            .or_else(|| text(d, "/license/id"))
    });
    dataset.updated =
        day(text(row, "/modified").or_else(|| text(row, "/issued")));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("europa.json"), 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "water".into(),
                url: "https://data.europa.eu/data/datasets/\
                      http-www-geoportal-rlp-de-registry-spatial-dataset-\
                      75be9c8c-b6d5-6e29-aca2-eb1ae907b29c"
                    .into(),
                description: Some(
                    "Topic Water supply from the spatial planning cadastre \
                     of the structural and permit diirersection south of \
                     the state of Rhineland-Palatinate"
                        .into()
                ),
                publisher: Some(
                    "SGD S\u{fc}d Raumordnung Technisches B\u{fc}ro".into()
                ),
                doi: None,
                license: Some(
                    "http://dcat-ap.de/def/licenses/dl-by-de/2.0".into()
                ),
                updated: Some("2023-12-06".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }
}
