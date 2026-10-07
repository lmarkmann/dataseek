//! Labelling: worksheets of every unjudged result any compared ranking puts
//! in its top 10, and folding the graded worksheets back into
//! judgments.tsv.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use dataseek::internals::Dataset;

use crate::compare::{configs, priors};
use crate::metrics::DEPTH;
use crate::snapshot::{self, Label, Query, parse_grade, tabless};
use crate::{cases, shipped, variant};

/// One worksheet per graded query with unjudged results, under
/// target/relevance-pool/. Rows are ordered by a hash of the link and carry
/// neither the source nor the rank, so the grader cannot tell which ranking
/// found a result or where.
pub fn pool(queries: &[Query]) -> Result<()> {
    let cases = cases(queries)?;
    let priors = priors(&cases);
    let configs = configs(&cases, &priors);
    let dir = snapshot::worksheets();
    fs::create_dir_all(&dir)?;
    let mut out = io::stdout().lock();
    let mut total = 0_usize;
    for case in cases.iter().filter(|c| c.query.graded()) {
        let mut rankings = vec![shipped(case)];
        rankings
            .extend(configs.iter().map(|(_, c)| variant(case, c, &priors)));
        let mut pending: BTreeMap<u64, Dataset> = BTreeMap::new();
        for hit in rankings.iter().flat_map(|r| r.iter().take(DEPTH)) {
            if case.judged.grade(&hit.dataset).is_none() {
                pending.insert(fnv(&hit.dataset.url), hit.dataset.clone());
            }
        }
        let path = dir.join(format!("{}.tsv", case.query.id));
        if pending.is_empty() {
            let _ = fs::remove_file(&path);
            continue;
        }
        let mut sheet = format!(
            "# {}\n# Fill in grade (0, 1 or 2) and a one-line reason; the rubric heads judgments.tsv.\n# grade\treason\turl\tdoi\ttitle\tpublisher\tdescription\n",
            case.query.text
        );
        for d in pending.values() {
            writeln!(
                sheet,
                "?\t\t{}\t{}\t{}\t{}\t{}",
                d.url,
                d.doi.as_deref().unwrap_or(""),
                tabless(&d.title),
                tabless(d.publisher.as_deref().unwrap_or("")),
                tabless(d.description.as_deref().unwrap_or(""))
            )?;
        }
        fs::write(&path, sheet)?;
        total = total.saturating_add(pending.len());
        writeln!(out, "{:<28} {:>3} to grade", case.query.id, pending.len())?;
    }
    writeln!(out, "{total} results to grade in {}", dir.display())?;
    Ok(())
}

/// FNV-1a: stable and seedless, so a worksheet's order is the same on
/// every machine.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Every graded worksheet row folded into judgments.tsv, replacing any
/// older label of the same dataset, and the file rewritten in order.
pub fn absorb(queries: &[Query]) -> Result<()> {
    let mut labels = snapshot::labels()?;
    let mut added = 0_usize;
    for q in queries {
        let path = snapshot::worksheets().join(format!("{}.tsv", q.id));
        let Some(text) = snapshot::read_if_present(&path)? else {
            continue;
        };
        let folded = fold(&mut labels, &q.id, &text, &path)?;
        added = added.saturating_add(folded);
    }
    snapshot::write_labels(queries, &mut labels)?;
    writeln!(
        io::stdout().lock(),
        "absorbed {added} labels; {} in total",
        labels.len()
    )?;
    Ok(())
}

/// The graded rows of one query's worksheet folded into `labels`: each
/// replaces every label of that query sharing an identity key with it.
/// Rows still `?` are skipped. Returns how many rows were folded in.
fn fold(
    labels: &mut Vec<Label>,
    query: &str,
    worksheet: &str,
    path: &Path,
) -> Result<usize> {
    let mut added = 0_usize;
    for (i, line) in worksheet.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let at = format!("{} line {}", path.display(), i.saturating_add(1));
        let fields: Vec<&str> = line.split('\t').collect();
        let &[grade, reason, url, doi, title, ..] = fields.as_slice() else {
            bail!("{at}: expected grade, reason, url, doi and title");
        };
        let Some(grade) = parse_grade(grade).context(at.clone())? else {
            continue;
        };
        if reason.trim().is_empty() {
            bail!("{at}: {url} has a grade but no reason");
        }
        let label = Label {
            query: query.to_owned(),
            grade: Some(grade),
            url: url.to_owned(),
            doi: doi.to_owned(),
            title: title.to_owned(),
            reason: reason.trim().to_owned(),
        };
        let keys = label.keys();
        labels.retain(|l| {
            l.query != query || !l.keys().iter().any(|k| keys.contains(k))
        });
        labels.push(label);
        added = added.saturating_add(1);
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::label;

    #[test]
    fn a_graded_row_replaces_the_label_sharing_its_identity() {
        let mut labels = vec![
            label("https://doi.org/10.1234/a", "10.1234/a", 0),
            label("https://x.org/b", "", 1),
        ];
        let sheet = "# q\n2\tthe data\thttps://repo.org/a\t10.1234/a\tA\n?\t\thttps://x.org/c\t\tC\n";
        let added = fold(&mut labels, "q", sheet, Path::new("q.tsv")).unwrap();
        assert_eq!(added, 1);
        let urls: Vec<&str> = labels.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, ["https://x.org/b", "https://repo.org/a"]);
        assert_eq!(labels[1].grade, Some(2));
    }

    #[test]
    fn a_short_row_or_an_unknown_grade_names_the_line() {
        let short =
            fold(&mut Vec::new(), "q", "# q\n2\treason\n", Path::new("q.tsv"))
                .unwrap_err();
        assert!(format!("{short:#}").contains("q.tsv line 2"), "{short:#}");
        let odd = fold(
            &mut Vec::new(),
            "q",
            "3\treason\thttps://x.org\t\tt\n",
            Path::new("q.tsv"),
        )
        .unwrap_err();
        let odd = format!("{odd:#}");
        assert!(
            odd.contains("q.tsv line 1") && odd.contains("\"3\""),
            "{odd}"
        );
    }
}
