//! Eurostat's table of contents, searched locally. Eurostat speaks SDMX, but
//! its dataflow list is 37 MB of multilingual annotations while the text
//! table of contents carries the same codes and English titles in 2 MB, so
//! that is what is downloaded. The XML one is 22 MB, ignores `lang`, and has
//! a short description for only 1,349 of its 10,311 datasets and tables
//! (Eurostat, October 2026).
//!
//! The text table of contents has no description column. It is a tree, a
//! folder's title names the topic of what sits in it, and the indentation of
//! a row (four spaces a level) gives its depth. A description is therefore
//! the folders above a dataset, without the root: "Population and social
//! conditions > Labour market > ...". A code can sit in several folders, and
//! the first one describes it. Comext and PRODCOM datasets (codes starting
//! `DS-`) are not listed; the Comext catalogue's table of contents answered
//! 404 (Eurostat, October 2026). No rate limit or key is published for the
//! catalogue API (Eurostat API documentation, October 2026).

use std::collections::HashSet;

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

const TOC: &str =
    "https://ec.europa.eu/eurostat/api/dissemination/catalogue/toc/txt";
const DATA_BROWSER: &str = "https://ec.europa.eu/eurostat/databrowser/view";

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let toc = ctx.http.get(TOC).query("lang", "en").slow().text()?;
    let entries = parse(&toc);
    if entries.is_empty() {
        return Err(SourceError::shape(
            "no datasets in the table of contents",
        ));
    }
    Ok(entries)
}

pub fn parse(toc: &str) -> Vec<Dataset> {
    let mut seen = HashSet::new();
    let mut folders: Vec<&str> = Vec::new();
    toc.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cells =
                line.split('\t').map(|c| c.trim().trim_matches('"'));
            let (indented, code, kind) =
                (cells.next()?, cells.next()?.trim(), cells.next()?.trim());
            let depth = indented.chars().take_while(|c| *c == ' ').count() / 4;
            folders.truncate(depth);
            let title = indented.trim();
            if kind == "folder" {
                folders.push(title);
                return None;
            }
            if !matches!(kind, "dataset" | "table") || !seen.insert(code) {
                return None;
            }
            let theme = folders
                .iter()
                .skip(1)
                .copied()
                .collect::<Vec<_>>()
                .join(" > ");
            let mut dataset = Dataset::new(
                title,
                &format!("{DATA_BROWSER}/{code}/default/table"),
            )
            .describe(Some(theme));
            dataset.publisher = Some("Eurostat".to_owned());
            dataset.updated = cells.next().and_then(european_date);
            dataset.valid()
        })
        .collect()
}

/// `"29.09.2026"` as `"2026-09-29"`.
fn european_date(raw: &str) -> Option<String> {
    let mut parts = raw.split('.');
    let (d, m, y) = (parts.next()?, parts.next()?, parts.next()?);
    let numeric = [d, m, y]
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    numeric.then(|| format!("{y}-{m}-{d}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    fn row(depth: usize, title: &str, code: &str, kind: &str) -> String {
        let indent = "    ".repeat(depth);
        format!(
            "\"{indent}{title}\"\t\"{code}\"\t\"{kind}\"\t\"29.09.2026\"\n"
        )
    }

    #[test]
    fn datasets_and_tables_are_kept_once_and_folders_dropped() {
        let toc = [
            "\"title\"\t\"code\"\t\"type\"\t\"last update of data\"\n"
                .to_owned(),
            row(0, "Database by themes", "data", "folder"),
            row(1, "Economy and finance", "economy", "folder"),
            row(2, "National accounts", "na10", "folder"),
            row(3, "Unemployment - monthly", "une_rt_m", "dataset"),
            row(3, "Unemployment - monthly", "une_rt_m", "dataset"),
            row(2, "Prices", "prc", "folder"),
            row(3, "GDP", "tec00001", "table"),
            row(1, "Loose dataset", "loose", "dataset"),
        ]
        .concat();
        let entries = parse(&toc);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].title, "Unemployment - monthly");
        assert_eq!(entries[0].updated.as_deref(), Some("2026-09-29"));
        assert!(entries[0].url.contains("/view/une_rt_m/"));
        assert_eq!(
            entries[0].description.as_deref(),
            Some("Economy and finance > National accounts")
        );
        assert_eq!(
            entries[1].description.as_deref(),
            Some("Economy and finance > Prices")
        );
        assert_eq!(entries[2].description, None);
    }

    #[test]
    fn records_map_from_a_recorded_list() {
        let datasets = parse(&fixture::text("eurostat.txt"));
        assert_eq!(datasets.len(), 4);
        assert_eq!(
            datasets[0],
            Dataset {
                title: "Current account - quarterly data".into(),
                url: "https://ec.europa.eu/eurostat/databrowser/view/\
                      ei_bpm6ca_q/default/table"
                    .into(),
                description: Some(
                    "General and regional statistics > European and \
                     national indicators for short-term analysis > Balance \
                     of payments"
                        .into()
                ),
                publisher: Some("Eurostat".into()),
                doi: None,
                license: None,
                updated: Some("2026-10-02".into()),
                size_bytes: None,
                popularity: None,
                aliases: vec![],
            }
        );
        assert_eq!(
            datasets[2].title,
            "Unemployment by sex and age - monthly data"
        );
        assert_eq!(
            datasets[2].description.as_deref(),
            Some(
                "Population and social conditions > Labour market > \
                 Employment and unemployment (Labour force survey) > LFS \
                 main indicators > Unemployment - LFS adjusted series"
            )
        );
        assert_eq!(
            datasets[3].title,
            "Employees by economic activity (NACE Rev. 2) (2008-2026)"
        );
        assert_eq!(
            datasets[3].description.as_deref(),
            Some(
                "Population and social conditions > Labour market > \
                 Employment and unemployment (Labour force survey) > Labour \
                 force survey (LFS) series - detailed annual data > Employees"
            )
        );
    }
}
