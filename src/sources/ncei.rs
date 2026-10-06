//! NOAA's National Centers for Environmental Information, through its
//! dataset search service (`v1`, the only version its documentation names).
//!
//! The service indexes 100 datasets, so a `limit` of 100, which is
//! `--per-source`'s ceiling, returns every match in one request (NCEI,
//! October 2026). `text` matches a dataset's name and description, any of the
//! words, and the best matches come first. The response has no
//! modification date: `endDate` is the end of the period of record, which
//! reads today's date for 34 of the 69 datasets matching "sea surface
//! temperature" and so is not `updated`. The documentation states no rate
//! limit.
//!
//! Any ASCII punctuation in `text` but `,` `-` `.` `:` and `_`, and any
//! non-ASCII letter, answers HTTP 400 "Invalid search options", so such a
//! query is asked once more as plain ASCII words (October 2026). `doiLink`
//! holds a DOI link for 62 of the 100 datasets and another landing page for
//! 36.

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let mut body = request(ctx, query, limit);
    if matches!(body, Err(SourceError::Status(400))) {
        let plain = plain_words(query);
        if !plain.is_empty() && plain != query {
            body = request(ctx, &plain, limit);
        }
    }
    parse(&body?, limit)
}

fn request(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Value, SourceError> {
    ctx.http
        .get("https://www.ncei.noaa.gov/access/services/search/v1/datasets")
        .query("text", query)
        .query("limit", limit)
        .json()
}

fn plain_words(query: &str) -> String {
    query
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn parse(
    body: &Value,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    if body.get("results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    Ok(items(body, "/results").iter().filter_map(record).take(limit).collect())
}

fn record(row: &Value) -> Option<Dataset> {
    let landing = text(row, "/doiLink")
        .or_else(|| text(row, "/links/other/0/url"))
        .or_else(|| {
            text(row, "/fileId").map(|file| {
                format!(
                    "https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id={file}"
                )
            })
        })?;
    let mut dataset = Dataset::new(&text(row, "/name")?, &landing)
        .describe(text(row, "/description"))
        .doi_from(text(row, "/doiLink"));
    dataset.publisher = text(row, "/organization/name");
    dataset.valid()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::json("ncei.json"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title:
                    "NOAA Optimum Interpolation 1/4 Degree Daily Sea Surface \
                        Temperature (OISST) Analysis, Version 2"
                        .into(),
                url: "https://doi.org/10.7289/V5SQ8XB5".into(),
                description: Some(
                    "This high-resolution sea surface temperature (SST) \
                     analysis product was developed using an optimum \
                     interpolation (OI) technique. The SST analysis has a \
                     spatial grid resolution of 0.25 (1/4) degree and \
                     temporal resolution of 1 day."
                        .into()
                ),
                publisher: Some(
                    "NOAA National Centers for Environmental Information"
                        .into()
                ),
                doi: Some("10.7289/v5sq8xb5".into()),
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_dataset_without_a_link_gets_its_landing_page_from_the_file_id() {
        let mut body = fixture::json("ncei.json");
        let row = &mut body["results"][0];
        row["doiLink"] = json!(null);
        row["links"] = json!({});
        let hits = parse(&body, 1).unwrap();
        assert_eq!(
            hits[0].url,
            "https://www.ncei.noaa.gov/access/metadata/landing-page/bin/\
             iso?id=gov.noaa.ncdc:C00844"
        );
        assert_eq!(hits[0].doi, None);
    }

    #[test]
    fn plain_words_keep_only_what_the_text_parameter_takes() {
        for (query, plain) in [
            ("O'Brien", "O Brien"),
            ("CO2 (ppm)", "CO2 ppm"),
            ("snow/ice", "snow ice"),
            ("\"sea surface\"", "sea surface"),
            ("sea*", "sea"),
            ("caf\u{e9}", "caf"),
            ("\u{65e5}\u{672c}\u{6d77}", ""),
        ] {
            assert_eq!(plain_words(query), plain, "{query}");
        }
    }
}
