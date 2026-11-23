//! Polaris Hub's drug-discovery benchmark datasets, read from the hub's
//! dataset list and searched locally. Each row carries its owner, tags,
//! license and the size of its Parquet table; the tags go before the
//! description so a teaser cut never drops them from the search text.
//!
//! One request of 100 rows holds every dataset (96, November 2026), and the
//! loop follows `metadata.total` past that. Each row also carries its whole
//! README and column annotations, so the page weighs 6 MB. The list comes
//! from polarishub.io: the api.polarishub.io host serves an expired
//! certificate. Rows are immutable versions with `updatedAt` always null,
//! so `createdAt` is the update date. robots.txt allows everything.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, number, text};

const PAGE: u64 = 100;

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    let mut offset: u64 = 0;
    for _ in 0..10 {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        let body = ctx
            .http
            .get("https://polarishub.io/api/v1/dataset")
            .query("limit", PAGE)
            .query("offset", offset)
            .slow()
            .json()?;
        let (page, total) = parse(&body)?;
        entries.extend(page);
        offset = offset.saturating_add(PAGE);
        if offset >= total {
            break;
        }
    }
    Ok(entries)
}

/// One page of datasets and the total the hub reports.
fn parse(body: &Value) -> Result<(Vec<Dataset>, u64), SourceError> {
    let total = number(body, "/metadata/total")
        .ok_or_else(|| SourceError::shape("no metadata.total"))?;
    let entries = items(body, "/data")
        .iter()
        .filter_map(|row| {
            let id = text(row, "/artifactId")?;
            let tags: Vec<&str> =
                items(row, "/tags").iter().filter_map(Value::as_str).collect();
            let about = text(row, "/description").unwrap_or_default();
            let description = if tags.is_empty() {
                about
            } else {
                format!("Tags: {}. {about}", tags.join(", "))
            };
            let mut dataset = Dataset::new(
                &text(row, "/name")?,
                &format!("https://polarishub.io/datasets/{id}"),
            )
            .describe(Some(description));
            dataset.publisher = text(row, "/owner/slug");
            dataset.license = text(row, "/license");
            dataset.updated = day(text(row, "/createdAt"));
            dataset.size_bytes = number(row, "/tableContent/size");
            dataset.formats =
                text(row, "/tableContent/fileType").into_iter().collect();
            dataset.valid()
        })
        .collect();
    Ok((entries, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let (hits, total) = parse(&fixture::json("polaris.json")).unwrap();
        assert_eq!(total, 96);
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "drewry2014-pkis1-subset-v2".into(),
                url: "https://polarishub.io/datasets/polaris/\
                      drewry2014-pkis1-subset-v2"
                    .into(),
                description: Some(
                    "Tags: kinase, hit-discovery, selectivity. A subset of \
                     PKIS dataset only including EGFR, RET, KIT kinases. \
                     PKIS is a data set of 367 small-molecule \
                     ATP-competitive kinase inhibitors that was screened by \
                     the set in activity assays with 224 recombinant \
                     kinases and 24 G protein-coupled receptors and in \
                     cellular assays of..."
                        .into()
                ),
                publisher: Some("polaris".into()),
                doi: None,
                license: Some("CC-BY-4.0".into()),
                updated: Some("2024-05-31".into()),
                size_bytes: Some(53_165),
                popularity: None,
                formats: vec!["parquet".into()],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_body_without_its_total_is_a_changed_shape() {
        let changed = serde_json::json!({"error": "unexpected"});
        assert!(matches!(parse(&changed), Err(SourceError::Shape(_))));
    }
}
