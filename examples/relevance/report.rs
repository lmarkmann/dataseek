//! The tables `check` and `variants` print.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Write;

use anyhow::Result;
use dataseek::internals::Hit;

use crate::metrics::{DEPTH, Interval, SEED, bootstrap, real};
use crate::{Baseline, Case, HELD, Metric, Part, Score, TUNE};

pub fn show(i: Interval) -> String {
    format!("{:.3} [{:.3}, {:.3}]", i.mean, i.low, i.high)
}

/// Each metric over all queries and each half, with its 95% interval.
pub fn summary(
    out: &mut impl Write,
    cases: &[Case],
    scores: &[Score],
) -> Result<()> {
    writeln!(
        out,
        "  {:<15} {:>28} {:>28} {:>28}",
        "metric", "all", "tune", "held"
    )?;
    for metric in Metric::ALL {
        let mut line = format!("  {:<15}", metric.label());
        for part in [Part::All, TUNE, HELD] {
            let values = metric.values(cases, scores, part);
            let i = bootstrap(&values, SEED, crate::metrics::RESAMPLES);
            let cell = format!("{} n={}", show(i), values.len());
            write!(line, " {cell:>28}")?;
        }
        writeln!(out, "{line}")?;
    }
    let known: Vec<&Score> = cases
        .iter()
        .zip(scores)
        .filter(|(c, _)| !c.query.graded())
        .map(|(_, s)| s)
        .collect();
    writeln!(
        out,
        "  {:<15} {} of {} known items in the top 10",
        "success@10",
        known.iter().filter(|s| s.found).count(),
        known.len()
    )?;
    Ok(())
}

/// How far the interval bounds move under two other seeds: the bootstrap's
/// own noise, which has to sit far below the gate's tolerance.
pub fn stability(
    out: &mut impl Write,
    cases: &[Case],
    scores: &[Score],
) -> Result<()> {
    let mut worst: f64 = 0.0;
    for metric in Metric::ALL {
        let values = metric.values(cases, scores, Part::All);
        let base = bootstrap(&values, SEED, crate::metrics::RESAMPLES);
        for seed in [SEED.wrapping_add(1), SEED.wrapping_mul(3)] {
            let other = bootstrap(&values, seed, crate::metrics::RESAMPLES);
            worst = worst
                .max((other.low - base.low).abs())
                .max((other.high - base.high).abs());
        }
    }
    writeln!(
        out,
        "  Interval bounds move by at most {worst:.4} under two other seeds."
    )?;
    Ok(())
}

/// Graded queries with no relevant result anywhere in their pool, and
/// top-10 slots left empty because fewer than ten results survived.
pub fn empty(
    out: &mut impl Write,
    cases: &[Case],
    rankings: &[Vec<Hit>],
) -> Result<()> {
    let graded: Vec<(&Case, &Vec<Hit>)> =
        cases.iter().zip(rankings).filter(|(c, _)| c.query.graded()).collect();
    let hopeless = graded
        .iter()
        .filter(|(c, _)| !c.judged.all().iter().any(|g| *g > 0))
        .count();
    let short: usize =
        graded.iter().map(|(_, r)| DEPTH.saturating_sub(r.len())).sum();
    writeln!(
        out,
        "  {hopeless} of {} graded queries have no relevant result in their pool; {short} top-10 slots are empty.",
        graded.len()
    )?;
    Ok(())
}

/// One ranking's top-10 slots over the graded queries: per source, the
/// slots it fills and how many of those hold a result judged not relevant;
/// and the totals, counting each slot once.
#[derive(Default)]
struct Tally {
    by_source: HashMap<&'static str, (usize, usize)>,
    slots: usize,
    bad: usize,
}

fn tally(cases: &[Case], rankings: &[Vec<Hit>]) -> Tally {
    let mut t = Tally::default();
    for (case, hits) in cases.iter().zip(rankings) {
        if !case.query.graded() {
            continue;
        }
        for hit in hits.iter().take(DEPTH) {
            let bad = usize::from(case.judged.grade(&hit.dataset) == Some(0));
            t.slots = t.slots.saturating_add(1);
            t.bad = t.bad.saturating_add(bad);
            for source in &hit.sources {
                let entry = t.by_source.entry(source).or_insert((0, 0));
                entry.0 = entry.0.saturating_add(1);
                entry.1 = entry.1.saturating_add(bad);
            }
        }
    }
    t
}

fn cell(filled: usize, bad: usize, slots: usize) -> String {
    let share = 100.0 * real(bad) / real(slots.max(1));
    format!("{filled:>4} {bad:>4} {share:>6.1}%")
}

/// Pollution of the top 10 over the graded queries, per source and per
/// ranking: the slots a source fills, how many of those hold a result
/// judged not relevant, and that count as a share of all top-10 slots. A
/// merged hit counts for each of its sources, so the shares add up to the
/// ranking's non-relevant rate only where no two sources share a slot.
/// Sorted by the first ranking's non-relevant count.
pub fn pollution(
    out: &mut impl Write,
    cases: &[Case],
    rankings: &[(&str, &[Vec<Hit>])],
) -> Result<()> {
    let tallies: Vec<Tally> =
        rankings.iter().map(|(_, r)| tally(cases, r)).collect();
    let Some(first) = tallies.first() else { return Ok(()) };
    let mut ids: Vec<&str> = tallies
        .iter()
        .flat_map(|t| t.by_source.keys().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    ids.sort_by_key(|id| {
        std::cmp::Reverse(first.by_source.get(id).map_or(0, |e| e.1))
    });
    writeln!(out, "Pollution of the top 10 over graded queries:")?;
    let mut head = format!("  {:<22}", "source");
    for (name, _) in rankings {
        write!(head, " {:>26}", format!("{name}: slots, bad, share"))?;
    }
    writeln!(out, "{head}")?;
    for id in ids.iter().take(12) {
        let mut line = format!("  {id:<22}");
        for t in &tallies {
            let (filled, bad) = t.by_source.get(id).copied().unwrap_or((0, 0));
            write!(line, " {:>26}", cell(filled, bad, t.slots))?;
        }
        writeln!(out, "{line}")?;
    }
    let mut line = format!("  {:<22}", "every slot");
    for t in &tallies {
        write!(line, " {:>26}", cell(t.slots, t.bad, t.slots))?;
    }
    writeln!(out, "{line}")?;
    Ok(())
}

/// The queries that moved most against the baseline, worst first.
pub fn moved(
    out: &mut impl Write,
    cases: &[Case],
    scores: &[Score],
    baseline: &Baseline,
) -> Result<()> {
    let mut changes: Vec<(f64, &str)> = cases
        .iter()
        .zip(scores)
        .filter_map(|(c, s)| {
            let was = baseline.queries.get(&c.query.id)?.first().copied()?;
            let now = if c.query.graded() { s.ndcg } else { s.rr };
            Some((now - was, c.query.id.as_str()))
        })
        .collect();
    changes.sort_by(|a, b| a.0.total_cmp(&b.0));
    writeln!(out, "\nLargest drops (nDCG@10, or RR for a known item):")?;
    for (change, id) in changes.iter().take(8).filter(|(c, _)| *c < 0.0) {
        writeln!(out, "  {id:<28} {change:+.4}")?;
    }
    Ok(())
}
