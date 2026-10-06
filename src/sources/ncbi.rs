//! NCBI GEO through E-utilities: `esearch` finds series and curated
//! DataSets, `esummary` fetches their records in one batch. NCBI asks every
//! client to send a tool name and contact address; a key raises the limit
//! from 3 to 10 requests a second.

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::{CONTACT, Call, SourceError};
use crate::record::{Dataset, day, items, number, text};

const EUTILS: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let found = polite(ctx, ctx.http.get(&format!("{EUTILS}/esearch.fcgi")))
        .query("db", "gds")
        .query("term", format!("({query}) AND (gse[ETYP] OR gds[ETYP])"))
        .query("retmax", limit)
        .json()?;
    let ids: Vec<String> = items(&found, "/esearchresult/idlist")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return if found.pointer("/esearchresult").is_some() {
            Ok(Vec::new())
        } else {
            Err(SourceError::shape("no esearchresult"))
        };
    }
    let summaries =
        polite(ctx, ctx.http.get(&format!("{EUTILS}/esummary.fcgi")))
            .query("db", "gds")
            .query("id", ids.join(","))
            .json()?;
    parse(&ids, &summaries, limit)
}

/// The esummary records for `ids`, in esearch's order.
pub(super) fn parse(
    ids: &[String],
    summaries: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if summaries.get("result").is_none() {
        return Err(SourceError::shape("no esummary result"));
    }
    Ok(ids
        .iter()
        .filter_map(|id| summaries.pointer(&format!("/result/{id}")))
        .filter_map(record)
        .take(limit)
        .collect())
}

fn polite<'a>(ctx: &Ctx<'_>, call: Call<'a>) -> Call<'a> {
    let call = call
        .query("retmode", "json")
        .query("tool", "dataseek")
        .query("email", CONTACT);
    match ctx.creds.get(Key::Ncbi) {
        Some(secret) => call.query("api_key", secret.token()),
        None => call,
    }
}

fn record(summary: &Value) -> Option<Dataset> {
    let accession = text(summary, "/accession")?;
    let mut dataset = Dataset::new(
        &text(summary, "/title")?,
        &format!(
            "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc={accession}"
        ),
    )
    .describe(text(summary, "/summary"));
    dataset.publisher = text(summary, "/taxon").map(|taxon| {
        match number(summary, "/n_samples") {
            Some(n) => format!("{taxon}, {n} samples"),
            None => taxon,
        }
    });
    dataset.updated = day(text(summary, "/pdat").map(|d| d.replace('/', "-")));
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    fn ids() -> Vec<String> {
        ["200304969", "200279746", "200279384"].map(String::from).to_vec()
    }

    #[test]
    fn records_map_from_a_recorded_summary() {
        let hits = parse(&ids(), &fixture::json("ncbi.json"), 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            Dataset {
                title: "scRNA seq of LUSC genetically engineered mouse models"
                    .into(),
                url: "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE304969"
                    .into(),
                description: Some(
                    "scRNA seq of PL(PTEN; LKB1) and PLA(PTEN:LKB1;ACKR3) LUSC \
                     genetically engineereed mouse"
                        .into()
                ),
                publisher: Some("Mus musculus, 4 samples".into()),
                updated: Some("2026-10-05".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn a_summary_without_results_is_a_shape_change() {
        let changed =
            parse(&ids(), &json!({"header": {"type": "esummary"}}), 10);
        assert!(matches!(changed, Err(SourceError::Shape(_))), "{changed:?}");
    }
}
