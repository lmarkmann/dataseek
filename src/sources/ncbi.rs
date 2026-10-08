//! NCBI GEO through E-utilities: `esearch` finds series and curated
//! DataSets, `esummary` fetches their records in one batch. NCBI asks every
//! client to send a tool name and contact address, registered with NCBI, and
//! allows 3 requests a second from one IP address, 10 with a key (NCBI
//! E-utilities guide, October 2026). One `esearch` returns up to 10,000 ids,
//! so a limit of 100 never needs a second page (NCBI, October 2026). The
//! esummary of a GEO record has no modification date, so `updated` is its
//! release date (NCBI, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::credentials::Key;
use crate::http::{CONTACT, Call, SourceError};
use crate::record::{Dataset, day, text};

const EUTILS: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const PUBLISHER: &str = "NCBI GEO";

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
    let ids = found_ids(&found)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let summaries =
        polite(ctx, ctx.http.get(&format!("{EUTILS}/esummary.fcgi")))
            .query("db", "gds")
            .query("id", ids.join(","))
            .json()?;
    parse(&ids, &summaries, limit)
}

/// The ids esearch matched, in its order. A query with no match answers
/// with an empty `idlist`; anything without one is a changed response.
fn found_ids(found: &Value) -> Result<Vec<String>, SourceError> {
    if let Some(refusal) = refusal(found) {
        return Err(refusal);
    }
    let ids = found
        .pointer("/esearchresult/idlist")
        .and_then(Value::as_array)
        .ok_or_else(|| SourceError::shape("no esearchresult idlist"))?;
    Ok(ids.iter().filter_map(Value::as_str).map(str::to_owned).collect())
}

/// What an E-utility said when it refused the request instead of answering:
/// `{"error": ...}` for a rate limit, `{"esearchresult": {"ERROR": ...}}`
/// for a bad request.
fn refusal(body: &Value) -> Option<SourceError> {
    let said =
        text(body, "/error").or_else(|| text(body, "/esearchresult/ERROR"))?;
    Some(if said.to_lowercase().contains("rate limit") {
        SourceError::RateLimited
    } else {
        SourceError::shape(said)
    })
}

/// The esummary records for `ids`, in esearch's order.
pub(super) fn parse(
    ids: &[String],
    summaries: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if let Some(refusal) = refusal(summaries) {
        return Err(refusal);
    }
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
        Some(secret) => call.key_query("api_key", secret.token()),
        None => call,
    }
}

fn landing_page(accession: &str) -> String {
    format!("https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc={accession}")
}

/// A curated DataSet (GDS) is built from one series (GSE); its `gse` names
/// that series, which the search may return as a record of its own.
fn record(summary: &Value) -> Option<Dataset> {
    let url = landing_page(&text(summary, "/accession")?);
    let mut dataset = Dataset::new(&text(summary, "/title")?, &url)
        .describe(text(summary, "/summary"));
    dataset.publisher = Some(PUBLISHER.to_owned());
    dataset.updated = day(text(summary, "/pdat").map(|d| d.replace('/', "-")));
    dataset.aliases = text(summary, "/gse")
        .map(|series| landing_page(&format!("GSE{series}")))
        .filter(|series_page| *series_page != url)
        .into_iter()
        .collect();
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    fn ids() -> Vec<String> {
        ["200304969", "200279746", "200279384", "5662"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn records_map_from_a_recorded_summary() {
        let hits = parse(&ids(), &fixture::json("ncbi.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
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
                publisher: Some("NCBI GEO".into()),
                updated: Some("2026-10-05".into()),
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn a_curated_dataset_points_at_the_series_it_was_built_from() {
        let hits = parse(&ids(), &fixture::json("ncbi.json"), 10).unwrap();
        assert_eq!(
            hits[3],
            Dataset {
                title: "Histone demethylase KDM3A-deficiency effect on \
                        estrogen-stimulated breast cancer cells in vitro"
                    .into(),
                url: "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GDS5662"
                    .into(),
                description: Some(
                    "Analysis of estrogen receptor (ER)-positive breast \
                     cancer cell line MCF-7 depleted for KDM3A (histone \
                     lysine demethylase 3A) then treated with estrogen. \
                     Histone lysine methylation is an important regulator of \
                     transcription. Results provide insight into role of \
                     KDM3A in ER signaling in breast cancer."
                        .into()
                ),
                publisher: Some("NCBI GEO".into()),
                updated: Some("2015-05-16".into()),
                aliases: vec![
                    "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE68918"
                        .into()
                ],
                ..Dataset::default()
            }
        );
    }

    #[test]
    fn a_series_and_the_dataset_curated_from_it_merge_into_one_hit() {
        let hits = parse(&ids(), &fixture::json("ncbi.json"), 10).unwrap();
        let series = Dataset {
            url: "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE68918"
                .into(),
            ..hits[0].clone()
        };
        let merged =
            crate::dedup::merge(vec![("geo", vec![series, hits[3].clone()])]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn a_summary_without_results_is_a_shape_change() {
        let changed =
            parse(&ids(), &json!({"header": {"type": "esummary"}}), 10);
        assert!(matches!(changed, Err(SourceError::Shape(_))), "{changed:?}");
    }

    #[test]
    fn a_search_with_no_match_is_an_empty_list() {
        let recorded = json!({
            "header": {"type": "esearch", "version": "0.3"},
            "esearchresult": {
                "count": "0", "retmax": "0", "retstart": "0", "idlist": [],
                "translationset": [],
                "querytranslation": "(zzqxjvkw9[All Fields]) AND (gse[ETYP] OR gds[ETYP])",
                "errorlist": {
                    "phrasesnotfound": ["zzqxjvkw9"], "fieldsnotfound": []
                },
                "warninglist": {
                    "phrasesignored": [], "quotedphrasesnotfound": [],
                    "outputmessages": ["No items found."]
                }
            }
        });
        assert_eq!(found_ids(&recorded).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_refused_search_is_an_error_not_an_empty_list() {
        let recorded = json!({
            "header": {"type": "esearch", "version": "0.3"},
            "esearchresult": {"ERROR": "Invalid db name specified: nosuchdb"}
        });
        let refused = found_ids(&recorded);
        assert!(matches!(refused, Err(SourceError::Shape(_))), "{refused:?}");
    }

    #[test]
    fn a_search_answering_without_an_idlist_is_a_shape_change() {
        let changed = found_ids(&json!({"esearchresult": {"count": "0"}}));
        assert!(matches!(changed, Err(SourceError::Shape(_))), "{changed:?}");
    }

    #[test]
    fn the_documented_rate_limit_body_is_rate_limited() {
        let documented =
            json!({"error": "API rate limit exceeded", "count": "11"});
        assert!(matches!(
            found_ids(&documented),
            Err(SourceError::RateLimited)
        ));
        assert!(matches!(
            parse(&ids(), &documented, 10),
            Err(SourceError::RateLimited)
        ));
    }
}
