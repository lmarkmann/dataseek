//! The CESSDA Data Catalogue: European social science archives (UKDS, GESIS,
//! FSD, SND, ...) harvested into one search. The API refuses requests without
//! a metadata language; English is asked for because it returns the most
//! studies (CESSDA, October 2026).
//!
//! Results come in relevance order. `limit` goes up to 200 and answers 400
//! above that. `offset + limit` may not pass 10,000, the index's result
//! window, so a page of up to 100 never needs paging (CESSDA, October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, day, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let body = ctx
        .http
        .get("https://datacatalogue.cessda.eu/api/DataSets/v2/search")
        .query("q", query)
        .query("limit", limit.clamp(1, 200))
        .query("metadataLanguage", "en")
        .json()?;
    parse(&body, limit)
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("Results").is_none() {
        return Err(SourceError::shape("no Results array"));
    }
    Ok(items(body, "/Results").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let id = text(row, "/id")?;
    let mut dataset = Dataset::new(
        &text(row, "/titleStudy")?,
        &format!("https://datacatalogue.cessda.eu/detail/{id}?lang=en"),
    )
    .describe(text(row, "/abstract"))
    .doi_from(items(row, "/pidStudies").iter().find_map(|pid| {
        text(pid, "/agency")
            .filter(|agency| agency.eq_ignore_ascii_case("doi"))
            .and(text(pid, "/pid"))
    }));
    dataset.publisher = text(row, "/publisher/publisher");
    dataset.updated = day(text(row, "/lastModified"));
    if let Some(study) = text(row, "/studyUrl") {
        dataset.aliases.push(study);
    }
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn the_doi_agency_is_matched_in_any_case() {
        // A pidStudies row as the Slovenian archive (ADP) sends it.
        let row = json!({
            "id": "x",
            "titleStudy": "t",
            "pidStudies": [
                {"agency": "ADP", "pid": "OOS23"},
                {"agency": "doi", "pid": "https://doi.org/10.17898/ADP_OOS23_V1"}
            ]
        });
        let dataset = record(&row).unwrap();
        assert_eq!(dataset.doi.as_deref(), Some("10.17898/adp_oos23_v1"));
    }

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("cessda.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(hits[3].doi.as_deref(), Some("10.5255/ukda-sn-856479"));
        assert_eq!(
            hits[0],
            Dataset {
                title: "European Climate Services User Survey (EU-MACS) 2017"
                    .into(),
                url: "https://datacatalogue.cessda.eu/detail/\
                      519d27e0cd34d4f7a43635cdb3a787e072646ba4e03d12900ad5b055caf00481\
                      ?lang=en"
                    .into(),
                description: Some(
                    "The survey was targeted at users and producers of \
                     climate services. It charted climate services available \
                     to Europeans as well as their use and development. The \
                     study was a part of the EU-MACS project funded by the \
                     European Commission (grant agreement ID: 730500)."
                        .into()
                ),
                publisher: Some("Finnish Social Science Data Archive".into()),
                doi: Some("10.60686/t-fsd3325".into()),
                license: None,
                updated: Some("2026-08-11".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec!["https://urn.fi/urn:nbn:fi:fsd:T-FSD3325".into()],
            }
        );
    }
}
