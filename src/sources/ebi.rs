//! EBI Search, the one search API over EMBL-EBI's archives. A [`Domain`]
//! picks the archive and the page prefix its accessions live under.
//!
//! The query is Lucene syntax with AND between words, and results come in
//! relevance order (EBI Search, October 2026). A page holds up to 1,000
//! entries, so one request covers any `--per-source`. A query Lucene cannot
//! parse ("tumor/normal", a trailing AND) answers HTTP 400, so that one
//! request is repeated once with the operator characters escaped. BioStudies
//! entries carry an abstract and no description, and `modified_date` was empty
//! on every entry sampled, so `updated` is the release date (October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, first_text, text};

pub struct Domain {
    pub name: &'static str,
    pub page: &'static str,
}

pub static ARRAYEXPRESS: Domain = Domain {
    name: "biostudies-arrayexpress",
    page: "https://www.ebi.ac.uk/biostudies/arrayexpress/studies/",
};
pub static BIOSTUDIES: Domain = Domain {
    name: "biostudies-other",
    page: "https://www.ebi.ac.uk/biostudies/studies/",
};

const OPERATORS: &str = r#"+-!(){}[]^"~*?:\/&|"#;

pub fn search(
    ctx: &Ctx<'_>,
    domain: &Domain,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let mut body = request(ctx, domain, query, limit);
    if matches!(body, Err(SourceError::Status(400))) {
        let plain = plain_words(query);
        if !plain.is_empty() && plain != query {
            body = request(ctx, domain, &plain, limit);
        }
    }
    parse(domain, &body?, limit)
}

fn request(
    ctx: &Ctx<'_>,
    domain: &Domain,
    query: &str,
    limit: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get(&format!(
            "https://www.ebi.ac.uk/ebisearch/ws/rest/{}",
            domain.name
        ))
        .query("query", query)
        .query("format", "json")
        .query("size", limit.clamp(1, 1000))
        .query("fields", "name,description,abstract,release_date")
        .json()
}

/// The words of a query with every Lucene operator character escaped and the
/// bare AND, OR and NOT and words made only of operator characters dropped, so
/// it always parses and no word is left that matches nothing.
fn plain_words(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|word| !matches!(*word, "AND" | "OR" | "NOT"))
        .filter(|word| !word.chars().all(|c| OPERATORS.contains(c)))
        .map(|word| {
            let mut escaped = String::with_capacity(word.len());
            for c in word.chars() {
                if OPERATORS.contains(c) {
                    escaped.push('\\');
                }
                escaped.push(c);
            }
            escaped
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn parse(
    domain: &Domain,
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let entries = body
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| SourceError::shape("no entries array"))?;
    Ok(entries
        .iter()
        .filter_map(|entry| record(domain, entry))
        .take(limit)
        .collect())
}

fn record(domain: &Domain, entry: &Value) -> Option<Dataset> {
    let accession = text(entry, "/acc")?;
    let mut dataset = Dataset::new(
        &text(entry, "/fields/name/0")?,
        &format!("{}{accession}", domain.page),
    )
    .describe(first_text(
        entry,
        &["/fields/description/0", "/fields/abstract/0"],
    ));
    dataset.updated = day(text(entry, "/fields/release_date/0"));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits =
            parse(&ARRAYEXPRESS, &fixture::json("ebi.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "High resolution analysis of genomic imprinting in the \
                        embryonic and adult mouse brain AND Sex-specific \
                        imprinting in the mouse brain"
                    .into(),
                url: "https://www.ebi.ac.uk/biostudies/arrayexpress/studies/\
                      E-GEOD-22131"
                    .into(),
                description: Some(
                    "Genomic imprinting results in the preferential \
                     expression of the paternal, or maternal allele of certain \
                     genes. We have performed a genome-wide characterization \
                     of imprinting in the mouse embryonic and adult brain \
                     using F1 hybrid mice generated from reciprocal crosses of \
                     CASTEiJ and C57BL/6J mice."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: Some("2010-07-08".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_biostudies_entry_without_a_description_shows_its_abstract() {
        let hits =
            parse(&BIOSTUDIES, &fixture::json("ebi.biostudies.json"), 10)
                .unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "TCF20 dysfunction leads to cortical neurogenesis \
                        defects and autistic-like behaviors in mice"
                    .into(),
                url: "https://www.ebi.ac.uk/biostudies/studies/\
                      S-SCDT-EMBOR-2019-49239V1"
                    .into(),
                description: Some(
                    "Recently, de novo mutations of transcription factor 20 \
                     (TCF20) were found in patients with autism by \
                     large-scale exome sequencing. However, how TCF20 \
                     modulates brain development and whether its dysfunction \
                     causes ASD remain unclear. Here, we show that TCF20 \
                     deficits impair neurogenesis in mouse."
                        .into()
                ),
                publisher: None,
                doi: None,
                license: None,
                updated: Some("2020-09-02".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_query_lucene_rejects_is_sent_again_as_plain_words() {
        assert_eq!(plain_words("tumor/normal"), r"tumor\/normal");
        assert_eq!(plain_words("10.1038/nature"), r"10.1038\/nature");
        assert_eq!(plain_words("[brain] (mouse)"), r"\[brain\] \(mouse\)");
        assert_eq!(plain_words("brain ~ mouse"), "brain mouse");
        assert_eq!(plain_words("C++"), r"C\+\+");
        assert_eq!(plain_words("mouse AND"), "mouse");
        assert_eq!(plain_words("NOT"), "");
    }
}
