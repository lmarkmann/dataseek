//! `bench`: ask every chosen source each query, uncached, and report per
//! source how often it answered, how fast (median and worst), how many
//! results it returned, how many merged hits only it found, and which other
//! source shares most of its hits. This is the measurement the ADR on source
//! coverage asks for before any source is demoted or dropped.
//!
//! Queries run one after another; sources run in parallel within a query,
//! exactly as `search` does, so the latencies are the ones a user sees.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

use crate::cli::Selection;
use crate::dedup::merge;
use crate::output::Out;
use crate::search::{Plan, run as search};
use crate::sources::{Services, select};
use crate::{palette, ui};

/// One query per broad category, so a default run touches every source's
/// home ground at least once.
const DEFAULT_QUERIES: [&str; 6] = [
    "climate temperature",
    "image classification",
    "unemployment rate",
    "gene expression cancer",
    "air quality",
    "population census",
];

#[derive(Serialize, Default)]
struct Row {
    id: &'static str,
    answered: usize,
    attempts: usize,
    median_ms: u128,
    worst_ms: u128,
    results: usize,
    unique: usize,
    contributed: usize,
    errors: Vec<String>,
    shared_with: BTreeMap<&'static str, usize>,
    #[serde(skip)]
    timings: Vec<Duration>,
}

pub fn run(
    queries: &[String],
    selection: &Selection,
    out: &Out,
) -> Result<()> {
    let services = Arc::new(Services::load()?);
    let sources =
        select(&selection.only, &selection.exclude, &selection.categories);
    let queries: Vec<String> = if queries.is_empty() {
        DEFAULT_QUERIES.iter().map(|q| (*q).to_owned()).collect()
    } else {
        queries.to_vec()
    };
    let mut rows: HashMap<&'static str, Row> = sources
        .iter()
        .map(|s| (s.id, Row { id: s.id, ..Row::default() }))
        .collect();

    let asked = sources
        .iter()
        .filter(|s| s.missing_key(&services.creds).is_none())
        .count();
    let total = queries.len().saturating_mul(asked);
    ui::stage(format!(
        "benchmarking {} sources on {} queries ({total} requests, no cache)",
        sources.len(),
        queries.len()
    ));
    let progress = ui::bar(total as u64, "requests");
    for query in &queries {
        let plan = Arc::new(Plan {
            query: query.clone(),
            sources: sources.clone(),
            per_source: usize::from(selection.per_source),
            forced: true,
            // The bench measures the source, not the cache's luck: each
            // catalog downloads here the way a named search would.
            named: sources.iter().map(|s| s.id.to_owned()).collect(),
        });
        let outcomes = search(&services, true, &plan, &progress, None);
        for o in &outcomes {
            let Some(row) = rows.get_mut(o.source.id) else { continue };
            if !o.status.attempted() {
                push_unique(&mut row.errors, o.status.label());
                continue;
            }
            row.attempts = row.attempts.saturating_add(1);
            row.timings.push(o.elapsed);
            if o.status.answered() {
                row.answered = row.answered.saturating_add(1);
                row.results = row.results.saturating_add(o.datasets.len());
            } else {
                push_unique(&mut row.errors, o.status.label());
            }
        }
        let lists =
            outcomes.into_iter().map(|o| (o.source.id, o.datasets)).collect();
        for hit in merge(lists) {
            for &id in &hit.sources {
                let Some(row) = rows.get_mut(id) else { continue };
                row.contributed = row.contributed.saturating_add(1);
                if hit.sources.len() == 1 {
                    row.unique = row.unique.saturating_add(1);
                }
                for &other in hit.sources.iter().filter(|o| **o != id) {
                    let count = row.shared_with.entry(other).or_insert(0);
                    *count = count.saturating_add(1);
                }
            }
        }
    }
    progress.finish_and_clear();
    services.cache.trim();

    let mut rows: Vec<Row> = rows.into_values().map(finish).collect();
    rows.sort_by(|a, b| {
        b.answered.cmp(&a.answered).then_with(|| a.median_ms.cmp(&b.median_ms))
    });
    print(out, &queries, &rows)
}

fn finish(mut row: Row) -> Row {
    row.timings.sort_unstable();
    row.median_ms = row
        .timings
        .get(row.timings.len().checked_div(2).unwrap_or(0))
        .map_or(0, Duration::as_millis);
    row.worst_ms = row.timings.last().map_or(0, Duration::as_millis);
    row
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

fn top_partner(row: &Row) -> Option<(&'static str, usize)> {
    row.shared_with
        .iter()
        .max_by_key(|(_, count)| **count)
        .map(|(id, count)| (*id, *count))
}

fn print(out: &Out, queries: &[String], rows: &[Row]) -> Result<()> {
    if out.json {
        return out.json(&serde_json::json!({
            "schema": "dataseek-bench/1",
            "queries": queries,
            "sources": rows,
        }));
    }
    let mut w = out.stdout();
    if out.plain {
        for r in rows {
            let partner =
                top_partner(r).map_or(String::new(), |(id, _)| id.to_owned());
            writeln!(
                w,
                "{}\t{}/{}\t{}\t{}\t{}\t{}\t{}",
                r.id,
                r.answered,
                r.attempts,
                r.median_ms,
                r.worst_ms,
                r.results,
                r.unique,
                partner
            )?;
        }
        return Ok(());
    }
    let (head, muted, warn) =
        (palette::accent(), palette::muted(), palette::warning());
    writeln!(
        w,
        "{head}{:<22} {:>7} {:>8} {:>8} {:>8} {:>7}  overlaps most with{head:#}",
        "source", "answer", "median", "worst", "results", "unique"
    )?;
    for r in rows {
        let partner = match top_partner(r) {
            Some((id, shared)) if r.contributed > 0 => {
                format!("{id} ({shared} of {})", r.contributed)
            }
            _ => String::new(),
        };
        writeln!(
            w,
            "{:<22} {:>3}/{:<3} {:>5} ms {:>5} ms {:>8} {:>7}  {partner}",
            r.id,
            r.answered,
            r.attempts,
            r.median_ms,
            r.worst_ms,
            r.results,
            r.unique
        )?;
        if !r.errors.is_empty() {
            writeln!(w, "{:<22} {warn}{}{warn:#}", "", r.errors.join("; "))?;
        }
    }
    writeln!(w)?;
    writeln!(
        w,
        "{muted}queries: {}. unique = merged hits no other source found.{muted:#}",
        queries.join(" | ")
    )?;
    Ok(())
}
