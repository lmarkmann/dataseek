//! Eurostat's table of contents, searched locally. Eurostat speaks SDMX,
//! but its dataflow list is 37 MB of multilingual annotations (2026-10-06)
//! while the table of contents carries the same codes and English titles in
//! 2 MB, so the table of contents is what is downloaded.

use std::collections::HashSet;

use super::Ctx;
use crate::http::SourceError;
use crate::record::Dataset;

pub fn list(ctx: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
    let toc = ctx
        .http
        .get("https://ec.europa.eu/eurostat/api/dissemination/catalogue/toc/txt")
        .query("lang", "en")
        .slow()
        .text()?;
    let entries = parse(&toc);
    if entries.is_empty() {
        return Err(SourceError::shape(
            "no datasets in the table of contents",
        ));
    }
    Ok(entries)
}

fn parse(toc: &str) -> Vec<Dataset> {
    let mut seen = HashSet::new();
    toc.lines()
        .skip(1)
        .filter_map(|line| {
            let cells: Vec<&str> =
                line.split('\t').map(|c| c.trim().trim_matches('"').trim()).collect();
            let (title, code, kind) = (cells.first()?, cells.get(1)?, cells.get(2)?);
            if !matches!(*kind, "dataset" | "table") || !seen.insert((*code).to_owned()) {
                return None;
            }
            let mut dataset = Dataset::new(
                title,
                &format!(
                    "https://ec.europa.eu/eurostat/databrowser/view/{code}/default/table"
                ),
            )
            .describe(Some((*code).to_owned()));
            dataset.publisher = Some("Eurostat".to_owned());
            dataset.updated = cells.get(3).and_then(|d| european_date(d));
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

    #[test]
    fn datasets_and_tables_are_kept_once_and_folders_dropped() {
        let toc = "\"title\"\t\"code\"\t\"type\"\t\"last update of data\"\n\
            \"Database by themes\"\t\"data\"\t\"folder\"\t\" \"\n\
            \"    Unemployment - monthly\"\t\"une_rt_m\"\t\"dataset\"\t\"29.09.2026\"\n\
            \"  Unemployment - monthly\"\t\"une_rt_m\"\t\"dataset\"\t\"29.09.2026\"\n\
            \"  GDP\"\t\"tec00001\"\t\"table\"\t\" \"\n";
        let entries = parse(toc);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Unemployment - monthly");
        assert_eq!(entries[0].updated.as_deref(), Some("2026-09-29"));
        assert!(entries[0].url.contains("/view/une_rt_m/"));
    }
}
