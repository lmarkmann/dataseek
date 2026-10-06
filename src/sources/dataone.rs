//! DataONE's coordinating-node Solr index over its member repositories (KNB,
//! Arctic Data Center, EDI, ...). Only current metadata records are asked
//! for, in Solr's relevance order, and a page holds up to 10000 (DataONE,
//! October 2026). The user's words are reduced to plain terms joined by AND,
//! and the words AND, OR and NOT are dropped, because Solr reads them as
//! operators and answers HTTP 400 when one lands between two others (DataONE,
//! October 2026). The index is reached through search.dataone.org:
//! cn.dataone.org renegotiates TLS on its `/cn/` paths to ask for an optional
//! client certificate, which rustls refuses by design.
//!
//! The DOI is the series id on about half of the current records, written
//! `doi:10.x/y` or `https://doi.org/10.x/y` (PANGAEA, Dryad, Dataverse, NSIDC,
//! ...), or the id itself (KNB, Arctic Data Center); every other id or series
//! id is a repository's own and is not a DOI (DataONE, October 2026).
//! `dateUploaded` is when the current version's content arrived, and content
//! never changes after upload; `dateModified` is when its system metadata,
//! such as access rules, last changed, so it is not asked for (DataONE,
//! October 2026). `origin` lists the creators, there is no publisher field,
//! and 4% of the records have no `origin` (DataONE, October 2026). DataONE
//! states no rate limit and no caching terms for the query service, and
//! each dataset carries its own license in its metadata, which the index
//! does not hold (DataONE, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, doi, items, text};

const OPERATORS: [&str; 3] = ["AND", "OR", "NOT"];

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let terms = terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let body = ctx
        .http
        .get("https://search.dataone.org/cn/v2/query/solr/")
        .query("q", terms.join(" AND "))
        .query("fq", "formatType:METADATA AND -obsoletedBy:*")
        .query(
            "fl",
            "id,seriesId,title,abstract,dateUploaded,datasource,origin,\
             contactOrganization",
        )
        .query("rows", limit)
        .query("wt", "json")
        .json()?;
    parse(&body, limit)
}

fn terms(query: &str) -> Vec<&str> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && !OPERATORS.contains(t))
        .collect()
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
    .describe(text(doc, "/abstract"));
    dataset.doi = doi_of(doc);
    dataset.publisher = text(doc, "/origin/0")
        .or_else(|| text(doc, "/contactOrganization/0"))
        .or_else(|| {
            text(doc, "/datasource")
                .map(|node| node.trim_start_matches("urn:node:").to_owned())
        });
    dataset.updated = day(text(doc, "/dateUploaded"));
    dataset.valid()
}

fn doi_of(doc: &Value) -> Option<String> {
    ["/seriesId", "/id"]
        .into_iter()
        .filter_map(|pointer| text(doc, pointer))
        .filter(|pid| is_doi(pid))
        .find_map(|pid| doi(&pid))
}

fn is_doi(pid: &str) -> bool {
    let lower = pid.to_ascii_lowercase();
    lower.starts_with("doi:") || lower.contains("doi.org/10.")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("dataone.json"), 10).unwrap();
        assert_eq!(hits.len(), 5);
        assert_eq!(
            hits[0],
            Dataset {
                title:
                    "Measurements of ice mass balance and temperature from \
                        autonomous Seasonal Ice Mass Balance (SIMB3) buoys \
                        deployed in the Central Arctic Ocean on the ArcWatch \
                        expedition in 2023"
                        .into(),
                url: "https://search.dataone.org/view/doi:10.18739/A21Z41V5P"
                    .into(),
                description: Some(
                    "The dataset contains measurements made from three \
                     autonomous Seasonal Ice Mass Balance (SIMB) buoys. The \
                     buoys were deployed in the late summer of 2023 as part \
                     of the Alfred Wegener Institute ArcWatch Project."
                        .into()
                ),
                publisher: Some("Donald Perovich".into()),
                doi: Some("10.18739/a21z41v5p".into()),
                license: None,
                updated: Some("2024-11-14".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn the_doi_comes_from_the_series_id_or_the_id() {
        let hits = parse(&fixture::json("dataone.json"), 10).unwrap();
        let dois: Vec<_> = hits.iter().map(|h| h.doi.as_deref()).collect();
        assert_eq!(
            dois,
            [
                Some("10.18739/a21z41v5p"),
                Some("10.5067/tlb86ryp2mry"),
                Some("10.7265/n5v122qs"),
                Some("10.7265/n57h1ggg"),
                None,
            ]
        );
        let series = json!({"id": "sha256:22074341", "seriesId": "doi:10.5067/C2HMG48G83QQ"});
        assert_eq!(doi_of(&series).as_deref(), Some("10.5067/c2hmg48g83qq"));
        let tdar = json!({"id": "doi:10.6067:XCV8KW5FJV_meta$v=1", "seriesId": "379627_meta"});
        assert_eq!(doi_of(&tdar), None);
    }

    #[test]
    fn a_record_without_origin_is_credited_to_its_contact_or_its_node() {
        let hits =
            parse(&fixture::json("dataone.no_origin.json"), 10).unwrap();
        let publishers: Vec<_> =
            hits.iter().map(|h| h.publisher.as_deref()).collect();
        let ncei = "DOC/NOAA/NESDIS/NCEI > National Centers for \
                    Environmental Information, NESDIS, NOAA, U.S. Department \
                    of Commerce";
        assert_eq!(publishers, [Some(ncei), Some(ncei), Some("C. Stephens")]);
        assert_eq!(hits[1].description, None);
        let hits = parse(&fixture::json("dataone.json"), 10).unwrap();
        assert_eq!(hits[4].publisher.as_deref(), Some("NPDC"));
    }

    #[test]
    fn solr_operators_never_reach_the_query() {
        assert_eq!(
            terms("land OR sea NOT ice, AND  M\u{fc}ller-or"),
            ["land", "sea", "ice", "M\u{fc}ller", "or"]
        );
        assert_eq!(terms("NOT AND OR"), Vec::<&str>::new());
    }
}
