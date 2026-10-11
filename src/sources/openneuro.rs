//! OpenNeuro's public datasets, paged out of its GraphQL API and searched
//! locally: the GraphQL `search` field returns null to anonymous clients
//! (OpenNeuro API, October 2026).
//!
//! The list is 1,905 datasets in 20 pages of 100 (October 2026), so `PAGES`
//! leaves room to triple. A public dataset without a snapshot comes back as
//! a null node beside an `errors` entry; it has nothing to show and is
//! dropped. `License` is the author's own text: mostly "CC0", sometimes a
//! whole license pasted in, which `LONGEST_LICENSE` keeps off the screen.

use serde_json::{Value, json};

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, from_markdown, items, number, text};

const PAGES: usize = 60;
const LONGEST_LICENSE: usize = 100;

const PAGE: &str = "query($after: String) { datasets(first: 100, after: $after, \
    filterBy: {public: true}) { pageInfo { hasNextPage endCursor } edges { node \
    { id analytics { downloads } latestSnapshot { created size readme \
    description { Name DatasetDOI License } } } } } }";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    let mut after = Value::Null;
    for _ in 0..PAGES {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        let body = ctx
            .http
            .post("https://openneuro.org/crn/graphql")
            .json_body(json!({"query": PAGE, "variables": {"after": after}}))
            .slow()
            .json()?;
        let (page, next) = parse(&body)?;
        entries.extend(page);
        match next {
            Some(cursor) => after = Value::String(cursor),
            None => break,
        }
    }
    Ok(entries)
}

/// One page of datasets and the cursor of the next page, if any.
pub(super) fn parse(
    body: &Value,
) -> Result<(Vec<Dataset>, Option<String>), SourceError> {
    let page = body
        .pointer("/data/datasets")
        .ok_or_else(|| SourceError::shape("no data.datasets"))?;
    let entries = items(page, "/edges").iter().filter_map(record).collect();
    let more = page.pointer("/pageInfo/hasNextPage").and_then(Value::as_bool);
    let next = match (more, text(page, "/pageInfo/endCursor")) {
        (Some(true), Some(cursor)) => Some(cursor),
        _ => None,
    };
    Ok((entries, next))
}

fn record(edge: &Value) -> Option<Dataset> {
    let id = text(edge, "/node/id")?;
    let name = text(edge, "/node/latestSnapshot/description/Name")
        .unwrap_or_else(|| id.clone());
    let mut dataset =
        Dataset::new(&name, &format!("https://openneuro.org/datasets/{id}"))
            .describe(
                text(edge, "/node/latestSnapshot/readme")
                    .map(|readme| from_markdown(&readme)),
            )
            .doi_from(text(
                edge,
                "/node/latestSnapshot/description/DatasetDOI",
            ));
    dataset.publisher = Some("OpenNeuro".to_owned());
    dataset.license = text(edge, "/node/latestSnapshot/description/License")
        .filter(|license| license.chars().count() <= LONGEST_LICENSE);
    dataset.updated = day(text(edge, "/node/latestSnapshot/created"));
    dataset.size_bytes = number(edge, "/node/latestSnapshot/size");
    dataset.popularity = number(edge, "/node/analytics/downloads");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let (entries, next) = parse(&fixture::json("openneuro.json")).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(next.as_deref(), Some("eyJvZmZzZXQiOjEwMH0="));
        assert_eq!(
            entries[0],
            Dataset {
                title: "Balloon Analog Risk-taking Task".into(),
                url: "https://openneuro.org/datasets/ds000001".into(),
                description: Some(
                    "This dataset was obtained from the OpenfMRI project \
                     (http://www.openfmri.org). Accession #: ds000001 \
                     Description: Balloon Analog Risk Task Please cite the \
                     following references if you use these data:"
                        .into()
                ),
                publisher: Some("OpenNeuro".into()),
                doi: Some("10.18112/openneuro.ds000001.v1.0.0".into()),
                license: Some("CC0".into()),
                updated: Some("2020-05-14".into()),
                size_bytes: Some(2_416_199_965),
                popularity: Some(2523),
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_license_pasted_as_a_paragraph_is_not_shown() {
        let (entries, _) = parse(&fixture::json("openneuro.json")).unwrap();
        assert_eq!(entries[1].title, "Classification learning");
        assert_eq!(entries[1].license, None);
        assert_eq!(entries[1].doi, None);
        assert_eq!(entries[1].popularity, Some(2944));
    }

    #[test]
    fn datasets_without_a_snapshot_are_dropped() {
        let body = fixture::json("openneuro.no_snapshot.json");
        let (entries, next) = parse(&body).unwrap();
        let ids: Vec<&str> = entries
            .iter()
            .map(|e| {
                e.url.trim_start_matches("https://openneuro.org/datasets/")
            })
            .collect();
        assert_eq!(ids, ["ds006663", "ds006670", "ds006673"]);
        assert_eq!(
            entries[0].doi.as_deref(),
            Some("10.18112/openneuro.ds006663.v1.0.3")
        );
        assert_eq!(entries[2].description, None);
        assert_eq!(next.as_deref(), Some("eyJvZmZzZXQiOjE2MDB9"));
    }
}
