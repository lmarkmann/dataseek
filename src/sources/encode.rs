//! ENCODE, the Encyclopedia of DNA Elements: ChIP-seq, ATAC-seq, RNA-seq,
//! Hi-C and other functional genomics experiments, searched through the
//! portal's REST API.
//!
//! `/search/` with `type=Experiment` and `searchTerm` matches every word of
//! the query against the experiment metadata, in the portal's relevance
//! order. `status=released` keeps archived and revoked experiments out.
//! `field=` names the properties the response carries; without it, 100
//! experiments with their files embedded weigh 1.7 MB, with it 22 KB
//! (live API, 2026-11-23). Quotes, colons and parentheses in a query do not
//! fail the search. A search with no hit answers HTTP 404 with a JSON body
//! whose `notification` is "No results found", so a 404 is an empty list.
//!
//! The terms limit use of the API "to 10 GET requests/sec from any single
//! user"; one search is one request (ENCODE REST API help, 2026-11-23).
//!
//! An experiment has no title of its own, so the title is built from its
//! target, assay and biosample, with the accession, because many
//! experiments share those three. Every experiment carries the DOI
//! `10.17989/<accession>`. A GEO series in `dbxrefs` becomes an alias, so
//! the same study from the `geo` source folds into this record.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

const FIELDS: [&str; 10] = [
    "accession",
    "assay_title",
    "target.label",
    "biosample_ontology.term_name",
    "description",
    "biosample_summary",
    "date_released",
    "dbxrefs",
    "doi",
    "lab.title",
];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let mut call = ctx
        .http
        .get("https://www.encodeproject.org/search/")
        .query("type", "Experiment")
        .query("status", "released")
        .query("format", "json")
        .query("searchTerm", query)
        .query("limit", limit.clamp(1, 100));
    for field in FIELDS {
        call = call.query("field", field);
    }
    match call.json() {
        Ok(body) => parse(&body, limit),
        Err(SourceError::Status(404)) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    super::parse_list(body, "/@graph", limit, record)
}

fn record(row: &Value) -> Option<Dataset> {
    let accession = text(row, "/accession")?;
    let mut dataset = Dataset::new(
        &title(row, &accession)?,
        &format!("https://www.encodeproject.org/experiments/{accession}/"),
    )
    .describe(text(row, "/description"))
    .doi_from(text(row, "/doi"));
    dataset.publisher = Some("ENCODE".to_owned());
    dataset.updated = day(text(row, "/date_released"));
    dataset.aliases = items(row, "/dbxrefs")
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|xref| xref.strip_prefix("GEO:"))
        .filter(|accession| accession.starts_with("GSE"))
        .map(|series| {
            format!(
                "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc={series}"
            )
        })
        .collect();
    dataset.valid()
}

fn title(row: &Value, accession: &str) -> Option<String> {
    let assay = text(row, "/assay_title")?;
    let assay = match text(row, "/target/label") {
        Some(target) => format!("{target} {assay}"),
        None => assay,
    };
    Some(match text(row, "/biosample_ontology/term_name") {
        Some(biosample) => format!("{assay} in {biosample} ({accession})"),
        None => format!("{assay} ({accession})"),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("encode.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "CTCF+RAD21 siRNA RNA-seq in K562 (ENCSR092LZV)".into(),
                url: "https://www.encodeproject.org/experiments/ENCSR092LZV/"
                    .into(),
                description: Some(
                    "RNA-seq on K562 cells treated with siRNA targeting \
                     both CTCF and RAD21 together."
                        .into()
                ),
                publisher: Some("ENCODE".into()),
                doi: Some("10.17989/encsr092lzv".into()),
                license: None,
                updated: Some("2016-10-09".into()),
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![
                    "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=\
                     GSE88020"
                        .into()
                ],
            }
        );
    }

    #[test]
    fn only_a_geo_series_becomes_an_alias() {
        let hits = parse(&fixture::json("encode.json"), 10).unwrap();
        assert_eq!(hits[3].title, "CTCF TF ChIP-seq in K562 (ENCSR000AKO)");
        assert!(hits[3].aliases.is_empty(), "GSM733719 is a sample");
    }

    #[test]
    fn a_title_does_without_a_target_or_a_biosample() {
        let bare = json!({"accession": "ENCSR1", "assay_title": "Hi-C"});
        assert_eq!(title(&bare, "ENCSR1").unwrap(), "Hi-C (ENCSR1)");
        let untargeted = json!({
            "assay_title": "ATAC-seq",
            "biosample_ontology": {"term_name": "liver"},
        });
        assert_eq!(
            title(&untargeted, "ENCSR2").unwrap(),
            "ATAC-seq in liver (ENCSR2)"
        );
    }
}
