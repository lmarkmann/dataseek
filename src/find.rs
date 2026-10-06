//! `search`: run the search loop, merge, and print. stdout carries the
//! merged results (text, `--plain` TSV, or `--json` with a per-source
//! report); stderr narrates progress and names the sources that failed.
//!
//! Terminal states: results found exits 0; nothing matched exits 0 with an
//! empty result set; some sources failing is a warning, not a failure; every
//! attempted source failing exits 1, because that is an offline machine or a
//! broken network rather than an empty answer.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::builder::styling::Style;
use serde::Serialize;

use crate::cli::Selection;
use crate::dedup::{Hit, merge, weigh};
use crate::output::Out;
use crate::search::{Outcome, Plan, Status, run};
use crate::sources::{Services, select};
use crate::{palette, ui};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "every source failed, so there is nothing to show\n  Try:   check the connection, then `dataseek search ... -v` for each source's error"
    )]
    AllFailed,
    #[error(
        "none of the chosen sources can run\n  Try:   `dataseek sources` shows which keys are missing"
    )]
    NothingRan,
}

pub struct Request<'a> {
    pub words: &'a [String],
    pub selection: &'a Selection,
    pub limit: usize,
    pub refresh: bool,
    /// Seconds to wait for the slowest sources; `None` waits for all.
    pub timeout: Option<u64>,
}

pub fn run_search(request: &Request<'_>, out: &Out) -> Result<()> {
    let Request { words, selection, limit, refresh, timeout } = *request;
    let services = Arc::new(Services::load()?);
    let query = words.join(" ");
    let plan = Arc::new(Plan {
        query: query.clone(),
        sources: select(
            &selection.only,
            &selection.exclude,
            &selection.categories,
        ),
        per_source: usize::from(selection.per_source),
        forced: !selection.only.is_empty(),
    });

    ui::stage(format!(
        "searching {} sources for \"{query}\"",
        plan.sources.len()
    ));
    let progress = ui::bar(plan.sources.len() as u64, "sources");
    let outcomes = run(
        &services,
        refresh,
        &plan,
        &progress,
        timeout.map(Duration::from_secs),
    );
    progress.finish_and_clear();
    services.cache.trim();

    narrate(out, &outcomes);
    if !outcomes.iter().any(|o| o.status.attempted()) {
        return Err(Error::NothingRan.into());
    }
    if !outcomes.iter().any(|o| o.status.answered()) {
        return Err(Error::AllFailed.into());
    }

    let lists: Vec<_> =
        outcomes.iter().map(|o| (o.source.id, o.datasets.clone())).collect();
    let mut hits = weigh(merge(&lists), &query);
    hits.truncate(limit);
    if hits.is_empty() {
        ui::warn(format!("no datasets matched \"{query}\""));
    }
    print(out, &query, &hits, &outcomes)
}

fn narrate(out: &Out, outcomes: &[Outcome]) {
    for o in outcomes {
        out.note(&format!(
            "{:<22} {:>5} ms  {:>3} results  {}",
            o.source.id,
            o.elapsed.as_millis(),
            o.datasets.len(),
            o.status.label()
        ));
    }
    let answered = outcomes.iter().filter(|o| o.status.answered()).count();
    let attempted = outcomes.iter().filter(|o| o.status.attempted()).count();
    ui::ok(format!("{answered} of {attempted} sources answered"));
    let failed: Vec<String> = outcomes
        .iter()
        .filter(|o| o.status.attempted() && !o.status.answered())
        .map(|o| format!("{} ({})", o.source.id, o.status.label()))
        .collect();
    if !failed.is_empty() {
        ui::warn(format!("no answer from {}", failed.join(", ")));
    }
    let downloading: Vec<&str> = outcomes
        .iter()
        .filter(|o| {
            o.source.is_catalog() && matches!(o.status, Status::Running(_))
        })
        .map(|o| o.source.id)
        .collect();
    if !downloading.is_empty() {
        ui::warn(format!(
            "{} were still downloading their catalogs; `dataseek cache warm` fetches them once",
            downloading.join(", ")
        ));
    }
}

#[derive(Serialize)]
struct Report<'a> {
    query: &'a str,
    results: &'a [Hit],
    sources: Vec<SourceReport>,
}

#[derive(Serialize)]
struct SourceReport {
    id: &'static str,
    status: String,
    answered: bool,
    results: usize,
    ms: u128,
}

fn print(
    out: &Out,
    query: &str,
    hits: &[Hit],
    outcomes: &[Outcome],
) -> Result<()> {
    let mut w = out.stdout();
    if out.json {
        let report = Report {
            query,
            results: hits,
            sources: outcomes
                .iter()
                .map(|o| SourceReport {
                    id: o.source.id,
                    status: o.status.label(),
                    answered: o.status.answered(),
                    results: o.datasets.len(),
                    ms: o.elapsed.as_millis(),
                })
                .collect(),
        };
        writeln!(w, "{}", serde_json::to_string(&report)?)?;
        return Ok(());
    }
    if out.plain {
        for hit in hits {
            let d = &hit.dataset;
            writeln!(
                w,
                "{}\t{}\t{}\t{}\t{}",
                tabless(&d.title),
                d.url,
                hit.sources.join(","),
                d.doi.as_deref().unwrap_or(""),
                d.updated.as_deref().unwrap_or("")
            )?;
        }
        return Ok(());
    }
    let (number, title, muted) =
        (palette::accent(), Style::new().bold(), palette::muted());
    let width = hits.len().to_string().len();
    for (i, hit) in hits.iter().enumerate() {
        let d = &hit.dataset;
        let pad = " ".repeat(width.saturating_add(2));
        writeln!(
            w,
            "{number}{:>width$}{number:#}  {title}{}{title:#}",
            i.saturating_add(1),
            d.title
        )?;
        writeln!(w, "{pad}{}", d.url)?;
        writeln!(w, "{pad}{muted}{}{muted:#}", facts(hit))?;
        if let Some(text) = &d.description {
            writeln!(w, "{pad}{text}")?;
        }
        writeln!(w)?;
    }
    Ok(())
}

/// The one-line provenance under a title: sources, publisher, date, size,
/// license, whichever are known.
fn facts(hit: &Hit) -> String {
    let d = &hit.dataset;
    let mut parts = vec![hit.sources.join(", ")];
    parts.extend(d.publisher.clone());
    parts.extend(d.updated.clone());
    parts.extend(d.size_bytes.map(human_bytes));
    parts.extend(d.license.clone());
    parts.extend(d.doi.as_ref().map(|doi| format!("doi:{doi}")));
    parts.join("  |  ")
}

#[expect(
    clippy::cast_precision_loss,
    reason = "display rounding; exactness past 2^53 bytes is irrelevant"
)]
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len().saturating_sub(1) {
        value /= 1000.0;
        unit = unit.saturating_add(1);
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {name}") }
}

fn tabless(text: &str) -> String {
    text.replace(['\t', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(155_173), "155.2 KB");
        assert_eq!(human_bytes(2_559_248_010_229), "2.6 TB");
    }
}
