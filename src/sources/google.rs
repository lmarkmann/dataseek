//! Google Dataset Search, read from the data its results page embeds.
//!
//! The page carries its first 20 results as one JSON array in an
//! `AF_initDataCallback({key: 'ds:0', ..., data: [...]})` block, the same data
//! the page renders from. Each result is a positional record; the field
//! positions below were mapped on 2026-10-06 and are the contract this
//! adapter depends on. Failsafes, in order: a consent or "unusual traffic"
//! interstitial is reported as [`SourceError::Blocked`]; a page without the
//! block, or results that no longer parse, is [`SourceError::Shape`] (the
//! search loop then serves the last cached answer); a record missing its
//! title or link is skipped rather than guessed at. One request per query:
//! the adapter never pages or retries against Google.
//!
//! Google documents no API and no paging (Google, October 2026): the
//! parameters `start`, `page` and `offset` return the same first 20, so a
//! query yields at most 20 results however large `limit` is. Google's terms
//! bar automated access only "in violation of the machine-readable
//! instructions on our web pages", and the host serves no `robots.txt`: it
//! answers 404 (Google terms effective July 2026; probed October 2026).

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, doi, first_text, number, text};

const BLOCK: &str = "AF_initDataCallback({key: 'ds:0'";

pub fn search(
    ctx: &Ctx<'_>,
    query: &str,
    limit: usize,
) -> Result<Vec<Dataset>, SourceError> {
    let page = ctx
        .http
        .get("https://datasetsearch.research.google.com/search")
        .query("query", query)
        .query("hl", "en")
        .text()?;
    parse(&page, limit)
}

fn parse(page: &str, limit: usize) -> Result<Vec<Dataset>, SourceError> {
    let Some(start) = page.find(BLOCK) else {
        let interstitial = page.contains("consent.google.com")
            || page.contains("unusual traffic")
            || page.contains("/sorry/");
        return Err(if interstitial {
            SourceError::Blocked
        } else {
            SourceError::shape("the results page has no ds:0 block")
        });
    };
    let block = page.get(start..).unwrap_or_default();
    let json = block
        .find("data:")
        .and_then(|at| block.get(at.saturating_add(5)..))
        .ok_or_else(|| SourceError::shape("the ds:0 block has no data"))?;
    let data = serde_json::Deserializer::from_str(json)
        .into_iter::<Value>()
        .next()
        .ok_or_else(|| SourceError::shape("the ds:0 data is empty"))?
        .map_err(|e| SourceError::shape(format!("ds:0 is not JSON: {e}")))?;

    let results = match data.get(1) {
        Some(Value::Array(results)) => results.as_slice(),
        Some(Value::Null) | None => &[],
        Some(_) => return Err(SourceError::shape("ds:0[1] is not a list")),
    };
    let datasets: Vec<Dataset> =
        results.iter().filter_map(record).take(limit).collect();
    if datasets.is_empty() && !results.is_empty() {
        return Err(SourceError::shape(format!(
            "none of {} results matched the known record layout",
            results.len()
        )));
    }
    Ok(datasets)
}

/// One result: `[_, _, record, docid, url]`; inside `record`, 1 is the
/// title, 2 the provider block (`[_, host, name, home, domain, icon,
/// [_, _, url]]`), 10 the file formats, 13 the license (`[[code, [url]]]`,
/// most records carrying the code alone), 21 the DOI, 25 the publishers
/// (`[[name, home, icon]]`), 27 the description (HTML), 32 the creators
/// (joined with `; `), 39 the last update ("Feb 17, 2026").
///
/// The publisher is Google's own publisher entry; without one, a single
/// creator (a Kaggle owner, a Mendeley author) stands in, and a list of
/// authors never does, so the provider takes over.
fn record(item: &Value) -> Option<Dataset> {
    let url = text(item, "/2/2/6/2").or_else(|| {
        text(item, "/4")
            .map(|u| u.split("#__").next().unwrap_or(&u).to_owned())
    })?;
    let mut dataset = Dataset::new(&text(item, "/2/1/0")?, &url)
        .describe(text(item, "/2/27/0/1"))
        .doi_from(text(item, "/2/21").as_deref().and_then(doi));
    dataset.publisher = text(item, "/2/25/0/0")
        .or_else(|| text(item, "/2/32").filter(|names| !names.contains(';')))
        .or_else(|| first_text(item, &["/2/2/2", "/2/2/1"]));
    dataset.license = text(item, "/2/13/0/1/0").or_else(|| {
        number(item, "/2/13/0/0").and_then(license_named).map(str::to_owned)
    });
    dataset.updated = text(item, "/2/39").map(|d| iso_date(&d).unwrap_or(d));
    dataset.size_bytes = text(item, "/2/10/0").as_deref().and_then(bytes_in);
    if let Some(page) = text(item, "/4") {
        dataset
            .aliases
            .push(page.split("#__").next().unwrap_or(&page).to_owned());
    }
    dataset.valid()
}

/// The license behind one of Google's own license codes, for results that
/// carry the code and no link. Each code was matched against the license the
/// provider's own metadata gives (Zenodo, Mendeley Data, Dataverse and
/// Figshare through DataCite, Kaggle through its API) on at least two
/// results in October 2026; a code seen less often stays unnamed.
fn license_named(code: u64) -> Option<&'static str> {
    match code {
        2 => Some("CC0-1.0"),
        8 => Some("CC-BY-4.0"),
        13 => Some("CC-BY-SA-4.0"),
        18 => Some("CC-BY-NC-4.0"),
        23 => Some("CC-BY-NC-SA-4.0"),
        33 => Some("CC-BY-NC-ND-4.0"),
        49 => Some("MIT"),
        50 => Some("Apache-2.0"),
        _ => None,
    }
}

/// `"Feb 17, 2026"` as `"2026-02-17"`.
fn iso_date(us: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct",
        "Nov", "Dec",
    ];
    let mut parts = us.split([' ', ',']).filter(|p| !p.is_empty());
    let month = parts.next()?;
    let day: u8 = parts.next()?.parse().ok()?;
    let year: u16 = parts.next()?.parse().ok()?;
    let index = MONTHS.iter().position(|m| month.starts_with(m))?;
    Some(format!("{year:04}-{:02}-{day:02}", index.saturating_add(1)))
}

/// `"zip(155173 bytes)"` as `155173`.
fn bytes_in(format: &str) -> Option<u64> {
    let inner = format.split_once('(')?.1;
    inner.split_once(" bytes")?.0.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    const PAGE: &str = r#"<script>AF_initDataCallback({key: 'ds:0', hash: '1', data:["sea ice",[[null,null,[null,["Arctic sea ice extent"],[null,"zenodo.org","Zenodo","https://zenodo.org","zenodo.org",null,[null,null,"https://zenodo.org/records/42"]],null,null,null,null,null,null,null,["zip(2048 bytes)"],null,null,null,null,null,null,null,null,null,null,"10.5281/zenodo.42",null,null,null,null,null,[[null,"<p>Daily extent</p>"]],null,null,null,null,"Polar Lab",null,null,null,null,null,null,"Feb 17, 2026"],"docid","https://zenodo.org/records/42#__sid=js0"],[null,null,[null,null]]],null,null,null,null,null,null,151,0], sideChannel: {}});</script>"#;

    #[test]
    fn records_parse_from_their_positions() {
        let hits = parse(PAGE, 10).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "the record without a title must be skipped"
        );
        let hit = &hits[0];
        assert_eq!(hit.title, "Arctic sea ice extent");
        assert_eq!(hit.url, "https://zenodo.org/records/42");
        assert_eq!(hit.doi.as_deref(), Some("10.5281/zenodo.42"));
        assert_eq!(hit.publisher.as_deref(), Some("Polar Lab"));
        assert_eq!(hit.description.as_deref(), Some("Daily extent"));
        assert_eq!(hit.updated.as_deref(), Some("2026-02-17"));
        assert_eq!(hit.size_bytes, Some(2048));
    }

    #[test]
    fn a_page_without_the_block_is_a_shape_change() {
        let err = parse("<html>new layout</html>", 10).unwrap_err();
        assert!(matches!(err, SourceError::Shape(_)), "{err:?}");
    }

    #[test]
    fn interstitials_are_reported_as_blocked() {
        let err = parse("Our systems have detected unusual traffic", 10)
            .unwrap_err();
        assert!(matches!(err, SourceError::Blocked), "{err:?}");
    }

    #[test]
    fn an_unrecognized_record_layout_is_not_an_empty_result() {
        let page =
            "AF_initDataCallback({key: 'ds:0', data:[\"q\",[[1,2,3]]]});";
        assert!(matches!(parse(page, 10), Err(SourceError::Shape(_))));
    }

    #[test]
    fn no_matches_is_an_empty_result() {
        let page = "AF_initDataCallback({key: 'ds:0', data:[\"q\",null]});";
        assert_eq!(parse(page, 10).unwrap().len(), 0);
    }

    #[test]
    fn us_dates_become_iso() {
        assert_eq!(iso_date("Feb 17, 2026").as_deref(), Some("2026-02-17"));
        assert_eq!(iso_date("Sept 1, 2013").as_deref(), Some("2013-09-01"));
        assert_eq!(iso_date("sometime"), None);
    }

    #[test]
    fn records_map_from_a_recorded_search() {
        let hits = parse(&fixture::text("google.html"), 10).unwrap();
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Sea Ice Index, Version 3".into(),
                url: "https://nsidc.org/data/g02135/versions/3".into(),
                description: Some(
                    "Notice: Due to funding limitations, this data set was \
                     recently changed to a \u{201c}Basic\u{201d} Level of \
                     Service. Learn more about what this means for users and \
                     how you can share your story here: Level of Service \
                     Update for Data Products."
                        .into()
                ),
                publisher: Some("National Snow and Ice Data Center".into()),
                doi: Some("10.7265/n5k072f8".into()),
                license: None,
                updated: Some("2019-08-13".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![
                    "https://nsidc.org/data/g02135/versions/3".into()
                ],
            }
        );
        let publishers: Vec<_> =
            hits[1..].iter().map(|h| h.publisher.as_deref()).collect();
        assert_eq!(
            publishers,
            [
                Some("Technical University of Denmark"),
                Some("NASA"),
                Some("willian oliveira"),
            ]
        );
        let licenses: Vec<_> =
            hits.iter().map(|h| h.license.as_deref()).collect();
        assert_eq!(
            licenses,
            [
                None,
                Some("CC-BY-4.0"),
                None,
                Some("https://creativecommons.org/publicdomain/zero/1.0/"),
            ]
        );
        assert_eq!(
            hits[2].aliases,
            vec![
                "https://data.nasa.gov/dataset/\
                 ease-grid-sea-ice-age-version-4-8b6b7"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn license_codes_and_author_lists_map_from_a_recorded_search() {
        let hits = parse(&fixture::text("google.codes.html"), 10).unwrap();
        assert_eq!(hits.len(), 7);
        assert_eq!(
            hits[0],
            Dataset {
                title: "Data from: Datasets for a data-centric image \
                        classification benchmark for noisy and ambiguous \
                        label estimation"
                    .into(),
                url: "https://datasetcatalog.nlm.nih.gov/dataset?q=0002202795"
                    .into(),
                description: Some(
                    "This is the official data repository of the \
                     Data-Centric Image Classification (DCIC) Benchmark. The \
                     goal of this benchmark is to measure the impact of \
                     tuning the dataset instead of the model for a variety \
                     of image classification datasets. Full details about \
                     the collection process, the structure"
                        .into()
                ),
                publisher: Some("Zenodo".into()),
                doi: Some("10.5281/zenodo.8115942".into()),
                license: None,
                updated: Some("2022-10-06".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![
                    "https://datasetcatalog.nlm.nih.gov/dataset?q=0002202795"
                        .into()
                ],
            }
        );
        let publishers: Vec<_> =
            hits.iter().map(|h| h.publisher.as_deref()).collect();
        assert_eq!(
            publishers,
            [
                Some("Zenodo"),
                Some("Ananya Verma"),
                Some("prasuldevv"),
                Some("Science Data Bank"),
                Some("abrahamchan2"),
                Some("Md Hasan Imam Bijoy"),
                Some("Sreevalli Manda"),
            ]
        );
        let licenses: Vec<_> =
            hits.iter().map(|h| h.license.as_deref()).collect();
        assert_eq!(
            licenses,
            [
                None,
                None,
                Some("MIT"),
                Some("CC-BY-4.0"),
                Some("Apache-2.0"),
                Some("CC-BY-NC-ND-4.0"),
                Some("https://creativecommons.org/publicdomain/zero/1.0/"),
            ]
        );
        let sizes: Vec<_> = hits.iter().map(|h| h.size_bytes).collect();
        assert_eq!(
            sizes,
            [
                None,
                Some(98_926_314),
                Some(1128),
                None,
                Some(241_294_629),
                None,
                Some(5_956_813),
            ]
        );
    }
}
