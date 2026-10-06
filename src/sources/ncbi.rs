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
    Ok(parse(&ids, &summaries, limit))
}

/// The esummary records for `ids`, in esearch's order.
pub(super) fn parse(
    ids: &[String],
    summaries: &Value,
    limit: usize,
) -> Vec<Dataset> {
    ids.iter()
        .filter_map(|id| summaries.pointer(&format!("/result/{id}")))
        .filter_map(record)
        .take(limit)
        .collect()
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
