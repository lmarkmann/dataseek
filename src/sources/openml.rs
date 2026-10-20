//! OpenML's active datasets, downloaded whole and searched locally. Only the
//! latest version of each name is kept, in the place its name first appears.
//!
//! The list comes from the Elasticsearch index behind openml.org, the one the
//! website's own search queries from the browser (`ELASTICSEARCH_SERVER` in
//! github.com/openml/openml.org, October 2026). It is not part of the
//! documented REST API, answers only POST (GET is HTTP 403), and speaks
//! Elasticsearch 6 (live, October 2026). The REST `data/list` it replaces
//! carries a name, a version and size qualities but no description, licence
//! or date (`data.tpl.php` in github.com/openml/OpenML, October 2026), and
//! failed with database error 107 "due to high server load" on the day the
//! index answered (October 2026).
//!
//! One request returns every active dataset: 6,668 documents, 10.7 MB in
//! 4.5 s (October 2026), against the index's `max_result_window` of 10,000.
//! Past that window the list would be cut silently; asking for more is HTTP
//! 500 (live, October 2026). Reads are limited to "~1000 requests/hour"
//! (openml.org/apis, October 2026), one request per catalog refresh.
//!
//! `licence` is free text ("Public" on 4,024 of 6,668, "CC0", "CC BY 4.0");
//! the values that name no licence are dropped. `last_update` is the
//! modification date. `runs`, the experiments run on the dataset, is the
//! popularity: filled for 2,327 datasets, against 1,408 with any download
//! count (October 2026). The index holds no DOI, citation or file size.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, from_markdown, items, number, text};

/// Lowercase `licence` values that say nothing about the licence.
const NO_LICENCE: [&str; 5] = [
    "na",
    "see source",
    "type license() to see the full license text",
    "undefined",
    "unknown",
];

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .post("https://www.openml.org/es/data/data/_search")
        .json_body(json!({
            "size": 10_000,
            "sort": [{"data_id": "asc"}],
            "_source": [
                "data_id",
                "name",
                "version",
                "description",
                "licence",
                "last_update",
                "runs",
                "qualities.NumberOfInstances",
                "qualities.NumberOfFeatures",
            ],
            "query": {"term": {"status": "active"}},
        }))
        .slow()
        .json()?;
    parse(&body)
}

pub(super) fn parse(body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = items(body, "/hits/hits");
    if rows.is_empty() {
        return Err(SourceError::shape("no hits.hits list"));
    }
    let mut latest: Vec<(u64, Dataset)> = Vec::new();
    let mut slot: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let Some(doc) = row.get("_source") else {
            continue;
        };
        let (Some(id), Some(name)) =
            (number(doc, "/data_id"), text(doc, "/name"))
        else {
            continue;
        };
        let version = number(doc, "/version").unwrap_or(0);
        let seen = slot.get(&name).copied();
        if seen.and_then(|i| latest.get(i)).is_some_and(|(v, _)| *v >= version)
        {
            continue;
        }
        let dataset = record(doc, id, &name);
        if let Some(older) = seen.and_then(|i| latest.get_mut(i)) {
            *older = (version, dataset);
        } else {
            slot.insert(name, latest.len());
            latest.push((version, dataset));
        }
    }
    Ok(latest.into_iter().map(|(_, d)| d).filter_map(Dataset::valid).collect())
}

/// The description leads with the table's shape, which no other field
/// carries. A description that only repeats the name is no description.
fn record(doc: &Value, id: u64, name: &str) -> Dataset {
    let shape = match (
        number(doc, "/qualities/NumberOfInstances"),
        number(doc, "/qualities/NumberOfFeatures"),
    ) {
        (Some(rows), Some(cols)) => {
            Some(format!("{rows} rows, {cols} features"))
        }
        _ => None,
    };
    let prose = text(doc, "/description")
        .filter(|d| d != name)
        .map(|d| from_markdown(&d));
    let description = match (shape, prose) {
        (Some(shape), Some(prose)) => Some(format!("{shape}. {prose}")),
        (shape, prose) => shape.or(prose),
    };
    let mut dataset =
        Dataset::new(name, &format!("https://www.openml.org/d/{id}"))
            .describe(description);
    dataset.license = text(doc, "/licence")
        .filter(|l| !NO_LICENCE.contains(&l.to_lowercase().as_str()));
    dataset.updated = day(text(doc, "/last_update"));
    dataset.popularity = number(doc, "/runs");
    dataset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::json("openml.json")).unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "anneal",
                "kr-vs-kp",
                "BNG(anneal,nominal,1000000)",
                "dating_profile",
                "18ProductivityPrediction",
            ]
        );
        assert_eq!(
            hits[0],
            Dataset {
                title: "anneal".into(),
                url: "https://www.openml.org/d/2".into(),
                description: Some(
                    "898 rows, 39 features. Author: Unknown. Donated by \
                     David Sterling and Wray Buntine Source: UCI - 1990 \
                     Please cite: UCI The original Annealing dataset from \
                     UCI. The exact meaning of the features and classes is \
                     largely unknown."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: Some("Public".into()),
                updated: Some("2015-09-02".into()),
                size_bytes: None,
                popularity: Some(13_779),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_latest_version_keeps_the_first_place_of_its_name() {
        let hits = parse(&fixture::json("openml.json")).unwrap();
        assert_eq!(
            hits[4],
            Dataset {
                title: "18ProductivityPrediction".into(),
                url: "https://www.openml.org/d/43250".into(),
                description: Some("1197 rows, 15 features".into()),
                publisher: None,
                doi: None,
                license: Some("Public".into()),
                updated: Some("2022-02-22".into()),
                size_bytes: None,
                popularity: Some(0),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_licence_that_names_none_is_dropped() {
        let hits = parse(&fixture::json("openml.json")).unwrap();
        assert_eq!(hits[3].title, "dating_profile");
        assert_eq!(hits[3].license, None, "the index says \"NA\"");
        assert_eq!(
            hits[3].description.as_deref(),
            Some(
                "59946 rows, 31 features. Anonymized data of dating \
                 profiles from OkCupid"
            )
        );
    }

    #[test]
    fn the_rest_list_shape_is_a_shape_error() {
        let rest =
            json!({"data": {"dataset": [{"did": 2, "name": "anneal"}]}});
        assert!(matches!(parse(&rest), Err(SourceError::Shape(_))));
    }
}
