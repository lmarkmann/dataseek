//! OmicsDI, the Omics Discovery Index across genomics, proteomics,
//! metabolomics and transcriptomics repositories.
//!
//! `start` and `size` page the search, and the docs cap a page at 100, which
//! is the most the CLI asks for, so one request fills any limit. The server
//! answers a larger `size` in full, but the clamp keeps to the documented 100
//! (OmicsDI API docs and live API, October 2026). Results come in the
//! server's default order, which the docs call relevance; `score` is null in
//! every hit. The query is Lucene syntax: an unbalanced quote or parenthesis
//! answers HTTP 404, and `word:` before a name that is not a field matches
//! nothing (live API, October 2026). No rate limit is published; EMBL-EBI's
//! terms say a user is blocked for a level of use that keeps the service from
//! others (EMBL-EBI terms of use, revised February 2024).
//!
//! The `biostudies-literature` source holds Europe PMC papers, each with
//! `omics_type` Unknown and no data of its own, yet it made up 22 to 42 of
//! 100 hits for three of five omics queries. The docs show only `AND`, but
//! the server also honours `NOT repository:"..."`, so the request leaves
//! those papers out (OmicsDI API docs and live API, October 2026).
//!
//! A search hit carries no DOI, license or size, and its counters are zero
//! for nearly every dataset. `publicationDate` is the only date it sends.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

/// Each source id OmicsDI answers with, and the repository it stands for
/// (`/ws/database/all`, October 2026).
const REPOSITORIES: [(&str, &str); 29] = [
    ("atlas-experiments", "Expression Atlas"),
    ("bioimages", "BioImages"),
    ("biomodels", "BioModels"),
    ("biostudies-arrayexpress", "ArrayExpress"),
    ("biostudies-literature", "BioStudies literature"),
    ("biostudies-other", "BioStudies"),
    ("cellcollective", "Cell Collective"),
    ("dbgap", "dbGaP"),
    ("ecrin-mdr-crc", "ECRIN MDR"),
    ("ega", "EGA"),
    ("eva", "EVA"),
    ("fairdomhub", "FAIRDOMHub"),
    ("geo", "GEO"),
    ("gnps", "GNPS"),
    ("gpmdb", "GPMDB"),
    ("iprox", "iProX"),
    ("jpost", "jPOST"),
    ("lincs", "LINCS"),
    ("massive", "MassIVE"),
    ("metabolights_dataset", "MetaboLights"),
    ("metabolomics_workbench", "Metabolomics Workbench"),
    ("ncbi", "NCBI"),
    ("node", "NODE"),
    ("panorama", "Panorama"),
    ("paxdb", "PaxDb"),
    ("peptide_atlas", "PeptideAtlas"),
    ("physiome", "Physiome Model Repository"),
    ("pride", "PRIDE"),
    ("project", "ENA"),
];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://www.omicsdi.org/ws/dataset/search")
        .query("query", without_literature(query))
        .query("size", limit.clamp(1, 100))
        .json()?;
    parse(&body, limit)
}

fn without_literature(query: &str) -> String {
    format!(r#"{query} NOT repository:"biostudies-literature""#)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("datasets").is_none() {
        return Err(SourceError::shape("no datasets array"));
    }
    Ok(items(body, "/datasets")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let source = text(row, "/source")?;
    let mut dataset = Dataset::new(
        &text(row, "/title")?,
        &format!("https://www.omicsdi.org/dataset/{source}/{id}"),
    )
    .describe(text(row, "/description"));
    dataset.publisher = Some(repository(&source));
    dataset.updated = day(text(row, "/publicationDate"));
    dataset.valid()
}

fn repository(source: &str) -> String {
    REPOSITORIES
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(source))
        .map_or_else(
            || source.replace('_', " "),
            |(_, name)| (*name).to_owned(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("omicsdi.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Distinct microbes, metabolites, and the host genome \
                        define the multi-omics profiles in right-sided and \
                        left-sided colon cancer"
                    .into(),
                url: "https://www.omicsdi.org/dataset/metabolights_dataset/\
                      MTBLS10232"
                    .into(),
                description: Some(
                    "BACKGROUND: Studies have reported clinical \
                     heterogeneity between right-sided colon cancer (RCC) \
                     and left-sided colon cancer (LCC). However, none of \
                     these studies used multi-omics analysis combining \
                     genetic regulation, microbiota and metabolites to \
                     explain the site-specific difference."
                        .into()
                ),
                publisher: Some("MetaboLights".into()),
                doi: None,
                license: None,
                updated: Some("2024-12-16".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(hits[2].title, "Breast Cancer RNA-seq");
        assert_eq!(
            hits[2].url,
            "https://www.omicsdi.org/dataset/biostudies-arrayexpress/\
             E-GEOD-58135"
        );
        assert_eq!(hits[2].publisher.as_deref(), Some("ArrayExpress"));
        assert_eq!(hits[2].updated.as_deref(), Some("2014-06-11"));
    }

    #[test]
    fn sources_are_named_for_their_repository() {
        assert_eq!(repository("project"), "ENA");
        assert_eq!(repository("iProX"), "iProX");
        assert_eq!(repository("new_repository"), "new repository");
    }

    #[test]
    fn the_query_leaves_out_literature() {
        assert_eq!(
            without_literature("cancer rna-seq"),
            r#"cancer rna-seq NOT repository:"biostudies-literature""#
        );
    }
}
