//! DataONE's coordinating-node Solr index over its member repositories (KNB,
//! Arctic Data Center, EDI, ...). Only current metadata records are asked
//! for; the user's words are reduced to plain terms so Solr syntax in a query
//! cannot break the request. The index is reached through
//! search.dataone.org: cn.dataone.org renegotiates TLS on its `/cn/` paths to
//! ask for an optional client certificate, which rustls refuses by design.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let body = ctx
        .http
        .get("https://search.dataone.org/cn/v2/query/solr/")
        .query("q", terms.join(" AND "))
        .query("fq", "formatType:METADATA AND -obsoletedBy:*")
        .query("fl", "id,title,abstract,dateModified,datasource,origin")
        .query("rows", limit)
        .query("wt", "json")
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/response/docs").is_none() {
        return Err(SourceError::shape("no response.docs"));
    }
    Ok(items(body, "/response/docs")
        .iter()
        .filter_map(record)
        .take(limit)
        .collect())
}

fn record(doc: &Value) -> Option<Dataset> {
    let id = text(doc, "/id")?;
    let mut dataset = Dataset::new(
        &text(doc, "/title")?,
        &format!("https://search.dataone.org/view/{id}"),
    )
    .describe(text(doc, "/abstract"))
    .doi_from(Some(id.clone()).filter(|i| i.starts_with("doi:")));
    dataset.publisher =
        text(doc, "/origin/0").or_else(|| text(doc, "/datasource"));
    dataset.updated = day(text(doc, "/dateModified"));
    dataset.valid()
}
