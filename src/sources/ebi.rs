//! EBI Search, the one search API over EMBL-EBI's archives. A [`Domain`]
//! picks the archive and the page prefix its accessions live under.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub struct Domain {
    pub name: &'static str,
    pub page: &'static str,
}

pub static ARRAYEXPRESS: Domain = Domain {
    name: "arrayexpress",
    page: "https://www.ebi.ac.uk/biostudies/arrayexpress/studies/",
};
pub static BIOSTUDIES: Domain = Domain {
    name: "biostudies-other",
    page: "https://www.ebi.ac.uk/biostudies/studies/",
};

pub fn search(
    ctx: &Ctx<'_>,
    domain: &Domain,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get(&format!(
            "https://www.ebi.ac.uk/ebisearch/ws/rest/{}",
            domain.name
        ))
        .query("query", query)
        .query("format", "json")
        .query("size", limit.clamp(1, 100))
        .query("fields", "name,description,publication_date")
        .json()?;
    if body.get("entries").is_none() {
        return Err(SourceError::shape("no entries array"));
    }
    Ok(items(&body, "/entries")
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
    .describe(text(entry, "/fields/description/0"));
    dataset.updated = day(text(entry, "/fields/publication_date/0"));
    dataset.valid()
}
