//! The NeurIPS Datasets and Benchmarks track, read from the yearly data
//! files of the conference's virtual site and searched locally. Each paper
//! has a title, an abstract, a page on the virtual site and, for most, a
//! project link and a proceedings page; the last two become aliases so the
//! paper merges with the dataset's own record elsewhere.
//!
//! A year's file holds every paper of every track (12 to 27 MB), and a row
//! belongs to the track when its `sourceurl` names the track's OpenReview
//! group. The 2021 file holds no row of the track, and in 2026 the track is
//! called Evaluations and Datasets. Accepted papers in the track: 163 in
//! 2022, 322 in 2023, 459 in 2024, 497 in 2025 and 970 in 2026, each once
//! although an oral is listed again as a poster (November 2026). The 2026
//! rows have no abstract yet. Only the records are cached, never the files.
//! robots.txt disallows `/static`, where the files sit, for crawlers; the
//! site's own pages load them, and one download a week is the whole load.

use std::collections::HashSet;

use serde_json::Value;

use super::Ctx;
use crate::http::SourceError;
use crate::record::{Dataset, items, text};

const YEARS: [u16; 5] = [2022, 2023, 2024, 2025, 2026];
/// The OpenReview group names of the track, old and new.
const TRACKS: [&str; 2] =
    ["Datasets_and_Benchmarks", "Evaluations_and_Datasets"];

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let mut entries = Vec::new();
    for year in YEARS {
        if ctx.stopped() {
            return Err(SourceError::Stopped);
        }
        let body = ctx
            .http
            .get(&format!(
                "https://neurips.cc/static/virtual/data/\
                 neurips-{year}-orals-posters.json"
            ))
            .slow()
            .json()?;
        entries.extend(parse(&body, year)?);
    }
    Ok(entries)
}

fn parse(body: &Value, year: u16) -> Result<Vec<Dataset>, SourceError> {
    if body.pointer("/results").is_none() {
        return Err(SourceError::shape("no results array"));
    }
    let mut seen = HashSet::new();
    Ok(items(body, "/results")
        .iter()
        .filter(|row| {
            text(row, "/sourceurl").is_some_and(|url| in_track(&url))
        })
        .filter_map(|row| {
            let title = text(row, "/name")?;
            if !seen.insert(title.clone()) {
                return None;
            }
            let page = text(row, "/virtualsite_url")?;
            let keywords: Vec<&str> = items(row, "/keywords")
                .iter()
                .filter_map(Value::as_str)
                .collect();
            let about = text(row, "/abstract").unwrap_or_default();
            let description = if keywords.is_empty() {
                about
            } else {
                format!("Keywords: {}. {about}", keywords.join(", "))
            };
            let mut dataset =
                Dataset::new(&title, &format!("https://neurips.cc{page}"))
                    .describe(Some(description));
            dataset.publisher = Some(format!("NeurIPS {year}"));
            dataset.aliases = ["/url", "/paper_url"]
                .iter()
                .filter_map(|pointer| text(row, pointer))
                .filter(|link| link.starts_with("http"))
                .collect();
            dataset.valid()
        })
        .collect())
}

/// Whether a `sourceurl` is the track's own OpenReview group. Rows of the
/// satellite site in Mexico City carry a `cdmx-` prefix and repeat papers
/// of other tracks, so they are left out.
fn in_track(source: &str) -> bool {
    source.starts_with("https://openreview.net/")
        && TRACKS.iter().any(|track| source.contains(track))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    #[test]
    fn records_map_from_a_recorded_list() {
        let hits = parse(&fixture::json("neurips.json"), 2024).unwrap();
        let titles: Vec<&str> =
            hits.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "HW-GPT-Bench: Hardware-Aware Architecture Benchmark for \
                 Language Models",
                "EffiBench: Benchmarking the Efficiency of Automatically \
                 Generated Code",
                "A Taxonomy of Challenges to Curating Fair Datasets",
            ],
            "the main-track paper and the repeated poster are dropped"
        );
        assert_eq!(hits[2].url, "https://neurips.cc/virtual/2024/oral/98019");
        assert_eq!(
            hits[0],
            Dataset {
                title: titles[0].into(),
                url: "https://neurips.cc/virtual/2024/poster/97460".into(),
                description: Some(
                    "The increasing size of language models necessitates a \
                     thorough analysis across multiple dimensions to assess \
                     trade-offs among crucial hardware metrics such as \
                     latency, energy consumption, GPU memory usage, and \
                     performance. Identifying optimal model configurations \
                     under specific hardware constraints is becoming..."
                        .into()
                ),
                publisher: Some("NeurIPS 2024".into()),
                doi: None,
                license: None,
                updated: None,
                size_bytes: None,
                popularity: None,
                formats: vec![],
                aliases: vec![
                    "https://github.com/automl/HW-GPT-Bench/".into(),
                    "https://proceedings.neurips.cc/paper_files/paper/2024/\
                     hash/6ffdbf064df51857eb802a904aaaba63-Abstract-\
                     Datasets_and_Benchmarks_Track.html"
                        .into(),
                ],
            }
        );
    }

    #[test]
    fn only_the_track_group_counts() {
        assert!(in_track(
            "https://openreview.net/group?id=NeurIPS.cc/2022/Track/\
             Datasets_and_Benchmarks"
        ));
        assert!(in_track(
            "https://openreview.net/group?id=NeurIPS.cc/2026/\
             Evaluations_and_Datasets_Track"
        ));
        assert!(!in_track(
            "cdmx-https://openreview.net/group?id=NeurIPS.cc/2025/\
             Datasets_and_Benchmarks_Track-cdmx"
        ));
        assert!(!in_track(
            "https://openreview.net/group?id=NeurIPS.cc/2024/Conference"
        ));
    }

    #[test]
    fn a_body_without_results_is_a_changed_shape() {
        let changed = serde_json::json!({"error": "unexpected"});
        assert!(matches!(parse(&changed, 2024), Err(SourceError::Shape(_))));
    }
}
