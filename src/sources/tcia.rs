//! The Cancer Imaging Archive's collections, read from the WordPress API of
//! its site and searched locally. Each collection carries its cancer types,
//! body locations, imaging modalities and other data types, species and
//! subject count; those lead the description, ahead of the abstract, so a
//! teaser cut never drops them from the search text.
//!
//! Pages of 100 hold 241 collections in three pages (November 2026), and
//! the loop asks for the next page only while a page is full. `_fields`
//! keeps each page near 340 KB instead of 2.2 MB. A collection marked
//! Limited needs registration and an approved license to download, and its
//! description says so. The legacy NBIA host, services.cancerimagingarchive.net, times out
//! and is not used. robots.txt disallows nothing.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, items, text};

const PAGE: usize = 100;
const FIELDS: &str = "link,collection_title,collection_short_title,\
    collection_doi,cancer_types,cancer_locations,data_types,supporting_data,\
    species,subjects,date_updated,collection_summary,collection_abstract,\
    collection_page_accessibility";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    for page in 1..=10 {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        let body = ctx
            .http
            .get("https://www.cancerimagingarchive.net/api/v1/collections/")
            .query("per_page", PAGE)
            .query("page", page)
            .query("_fields", FIELDS)
            .slow()
            .json()?;
        let rows = body.as_array().ok_or_else(|| {
            SourceError::shape("expected a list of collections")
        })?;
        entries.extend(rows.iter().filter_map(collection));
        if rows.len() < PAGE {
            break;
        }
    }
    Ok(entries)
}

fn collection(row: &Value) -> Option<Dataset> {
    let listed = |pointer: &str| -> Vec<&str> {
        items(row, pointer).iter().filter_map(Value::as_str).collect()
    };
    let data = [listed("/data_types"), listed("/supporting_data")].concat();
    let facts = [
        text(row, "/collection_title"),
        labelled("Cancer", &listed("/cancer_types")),
        labelled("Location", &listed("/cancer_locations")),
        labelled("Data", &data),
        labelled("Species", &listed("/species")),
        text(row, "/subjects").map(|n| format!("{n} subjects")),
        (text(row, "/collection_page_accessibility").as_deref()
            == Some("Limited"))
        .then(|| "Access: Limited".to_owned()),
        first_text(row, &["/collection_abstract", "/collection_summary"]),
    ];
    let description: Vec<String> = facts.into_iter().flatten().collect();
    let mut dataset = Dataset::new(
        &first_text(row, &["/collection_short_title", "/collection_title"])?,
        &text(row, "/link")?,
    )
    .describe(Some(description.join(". ")))
    .doi_from(text(row, "/collection_doi"));
    dataset.updated = day(text(row, "/date_updated"));
    dataset.valid()
}

fn labelled(label: &str, values: &[&str]) -> Option<String> {
    (!values.is_empty()).then(|| format!("{label}: {}", values.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let body = fixture::json("tcia.json");
        let hits: Vec<Dataset> =
            body.as_array().unwrap().iter().filter_map(collection).collect();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].title, "FHCC-PTRC-Melanoma");
        assert_eq!(
            hits[1],
            Dataset {
                title: "CFB-GBM".into(),
                url: "https://www.cancerimagingarchive.net/collection/\
                      cfb-gbm/"
                    .into(),
                description: Some(
                    "Pre and post treatment MRI and radiotherapy plans of \
                     patients with glioblastoma: the CFB-GBM cohort. Cancer: \
                     Glioblastoma. Location: Brain. Data: CT, MR, \
                     Segmentation, Software/Source Code. Species: Human. 264 \
                     subjects. The \u{201c}Centre Fran\u{e7}ois Baclesse \
                     glioblastoma (CFB-GBM)\u{201d} dataset includes 264 patients \
                     with..."
                        .into()
                ),
                publisher: None,
                doi: Some("10.7937/v9pn-2f72".into()),
                license: None,
                updated: Some("2026-09-11".into()),
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![],
            }
        );
    }
}
