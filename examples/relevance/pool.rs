//! Labelling: worksheets of every unjudged result any compared ranking puts
//! in its top 10, and folding the graded worksheets back into
//! judgments.tsv.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};

use anyhow::{Result, bail};
use dataseek::internals::Dataset;

use crate::compare::{configs, priors};
use crate::metrics::{DEPTH, Grade};
use crate::snapshot::{self, Label, Query, tabless};
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

/// Every graded worksheet row folded into judgments.tsv, replacing an older
/// label of the same record, and the file rewritten in order.
pub fn absorb(queries: &[Query]) -> Result<()> {
    let mut labels = snapshot::labels()?;
    let mut added = 0_usize;
    for q in queries {
        let path = snapshot::worksheets().join(format!("{}.tsv", q.id));
        let Ok(text) = fs::read_to_string(&path) else { continue };
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let fields: Vec<&str> = line.split('\t').collect();
            let &[grade, reason, url, doi, title, ..] = fields.as_slice()
            else {
                continue;
            };
            let grade: Grade = match grade.trim() {
                "0" => 0,
                "1" => 1,
                "2" => 2,
                _ => continue,
            };
            if reason.trim().is_empty() {
                bail!("{}: {url} has a grade but no reason", path.display());
            }
            labels.retain(|l| !(l.query == q.id && l.url == url));
            labels.push(Label {
                query: q.id.clone(),
                grade: Some(grade),
                url: url.to_owned(),
                doi: doi.to_owned(),
                title: title.to_owned(),
                reason: reason.trim().to_owned(),
            });
            added = added.saturating_add(1);
        }
    }
    snapshot::write_labels(queries, &mut labels)?;
    writeln!(
        io::stdout().lock(),
        "absorbed {added} labels; {} in total",
        labels.len()
    )?;
    Ok(())
}
