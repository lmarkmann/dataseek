//! The relevance benchmark: how good the merged top 10 is, measured offline
//! from per-source lists recorded once and from graded judgments.
//!
//! ```text
//! just relevance            score the shipped ranking; fail on a regression
//! just relevance --bless    accept the current scores as the baseline
//! just relevance variants   compare every ranking variant, tune and held out
//! just relevance pool       worksheets for every unjudged top-10 result
//! just relevance absorb     fold filled-in worksheets into judgments.tsv
//! just relevance record     re-record the snapshot from the live sources
//! ```
//!
//! docs/reference/development.md has the workflow.

mod compare;
mod metrics;
mod pool;
mod record;
mod report;
mod snapshot;
mod variants;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dataseek::internals::{Dataset, Hit, merge, weigh};
use serde::{Deserialize, Serialize};

use metrics::{DEPTH, Grade, SEED, bootstrap, mean, real};
use snapshot::{Answered, Half, Judged, Lists, Query};
use variants::{Config, Priors};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let queries = snapshot::queries()?;
    match args.first().map(String::as_str) {
        None | Some("check") => check(&queries, false),
        Some("--bless") => check(&queries, true),
        Some("variants") => compare::run(&queries),
        Some("pool") => pool::pool(&queries),
        Some("absorb") => pool::absorb(&queries),
        Some("record") => record::run(&queries, args.get(1..).unwrap_or(&[])),
        Some(other) => bail!(
            "unknown mode {other:?}\n  Try:   check, --bless, variants, pool, absorb or record"
        ),
    }
}

/// One query with everything needed to score a ranking of it.
pub(crate) struct Case {
    pub(crate) query: Query,
    pub(crate) lists: Lists,
    /// How each source answered when the snapshot was recorded.
    pub(crate) sources: Vec<Answered>,
    pub(crate) judged: Judged,
    pub(crate) targets: Vec<String>,
}

pub(crate) fn cases(queries: &[Query]) -> Result<Vec<Case>> {
    let labels = snapshot::labels()?;
    queries
        .iter()
        .map(|q| {
            let retrieval = snapshot::load(q)?;
            Ok(Case {
                lists: retrieval.lists,
                sources: retrieval.sources,
                judged: Judged::new(&labels, &q.id)?,
                targets: snapshot::target_keys(q),
                query: q.clone(),
            })
        })
        .collect()
}

/// The ranking a user sees today.
pub(crate) fn shipped(case: &Case) -> Vec<Hit> {
    weigh(merge(case.lists.clone()), &case.query.text)
}

pub(crate) fn variant(
    case: &Case,
    config: &Config,
    priors: &Priors,
) -> Vec<Hit> {
    let hits = variants::candidates(&case.lists, config);
    let order =
        variants::rank(&hits, &case.lists, &case.query.text, config, priors);
    let mut slots: Vec<Option<Hit>> = hits.into_iter().map(Some).collect();
    order.iter().filter_map(|&i| slots.get_mut(i)?.take()).collect()
}

/// How one ranking did on one query.
#[derive(Clone, Debug, Default)]
pub(crate) struct Score {
    pub(crate) ndcg: f64,
    pub(crate) precision: f64,
    pub(crate) rr: f64,
    pub(crate) found: bool,
    /// Top-10 results with no grade yet.
    pub(crate) unjudged: Vec<Dataset>,
}

pub(crate) fn score(case: &Case, ranked: &[Hit]) -> Score {
    if case.query.graded() {
        graded(&case.judged, ranked)
    } else {
        let is_target: Vec<bool> = ranked
            .iter()
            .map(|h| snapshot::is_target(h, &case.targets))
            .collect();
        Score {
            rr: metrics::reciprocal_rank(&is_target),
            found: is_target.iter().take(DEPTH).any(|t| *t),
            ..Score::default()
        }
    }
}

/// nDCG@10 and P@10 against the labels; an unjudged result counts as not
/// relevant and is reported.
pub(crate) fn graded(judged: &Judged, ranked: &[Hit]) -> Score {
    let mut unjudged = Vec::new();
    let grades: Vec<Grade> = ranked
        .iter()
        .take(DEPTH)
        .map(|h| {
            judged.grade(&h.dataset).unwrap_or_else(|| {
                unjudged.push(h.dataset.clone());
                0
            })
        })
        .collect();
    Score {
        ndcg: metrics::ndcg(&grades, &judged.all()),
        precision: metrics::precision(&grades),
        unjudged,
        ..Score::default()
    }
}

/// Which queries a summary covers.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Part {
    All,
    Half(Half),
}

pub(crate) const TUNE: Part = Part::Half(Half::Tune);
pub(crate) const HELD: Part = Part::Half(Half::Held);

impl Part {
    fn holds(self, q: &Query) -> bool {
        match self {
            Self::All => true,
            Self::Half(h) => q.half == h,
        }
    }
}

#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Metric {
    Ndcg,
    Precision,
    Mrr,
}

impl Metric {
    pub(crate) const ALL: [Self; 3] = [Self::Ndcg, Self::Precision, Self::Mrr];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ndcg => "nDCG@10",
            Self::Precision => "P@10",
            Self::Mrr => "known-item MRR",
        }
    }

    /// The per-query values this metric averages, over the queries within
    /// `part` it applies to: graded queries for nDCG and P@10, known items
    /// for MRR.
    pub(crate) fn values(
        self,
        cases: &[Case],
        scores: &[Score],
        part: Part,
    ) -> Vec<f64> {
        cases
            .iter()
            .zip(scores)
            .filter(|(c, _)| part.holds(&c.query))
            .filter(|(c, _)| c.query.graded() == (self != Self::Mrr))
            .map(|(_, s)| match self {
                Self::Ndcg => s.ndcg,
                Self::Precision => s.precision,
                Self::Mrr => s.rr,
            })
            .collect()
    }

    pub(crate) fn mean(
        self,
        cases: &[Case],
        scores: &[Score],
        part: Part,
    ) -> f64 {
        mean(&self.values(cases, scores, part))
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Baseline {
    pub(crate) metrics: BTreeMap<Metric, Gate>,
    /// Per query: nDCG@10 and P@10, or the known item's reciprocal rank.
    pub(crate) queries: BTreeMap<String, Vec<f64>>,
}

/// One metric's committed mean, its bootstrap interval, and the drop the
/// gate tolerates: a quarter of its standard error.
#[derive(Serialize, Deserialize)]
pub(crate) struct Gate {
    pub(crate) mean: f64,
    pub(crate) low: f64,
    pub(crate) high: f64,
    pub(crate) tolerance: f64,
}

fn baseline_path() -> PathBuf {
    snapshot::dir().join("baseline.json")
}

/// The committed baseline; `None` only when there is none yet.
pub(crate) fn baseline() -> Result<Option<Baseline>> {
    baseline_at(&baseline_path())
}

fn baseline_at(path: &Path) -> Result<Option<Baseline>> {
    let Some(text) = snapshot::read_if_present(path)? else {
        return Ok(None);
    };
    let baseline = serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(baseline))
}

fn round(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn gates(cases: &[Case], scores: &[Score]) -> BTreeMap<Metric, Gate> {
    Metric::ALL
        .iter()
        .map(|&m| {
            let values = m.values(cases, scores, Part::All);
            let i = bootstrap(&values, SEED, metrics::RESAMPLES);
            let gate = Gate {
                mean: round(i.mean),
                low: round(i.low),
                high: round(i.high),
                tolerance: round(i.se / 4.0),
            };
            (m, gate)
        })
        .collect()
}

/// Per query: nDCG@10 and P@10, or the known item's reciprocal rank.
fn per_query(cases: &[Case], scores: &[Score]) -> BTreeMap<String, Vec<f64>> {
    cases
        .iter()
        .zip(scores)
        .map(|(c, s)| {
            let values = if c.query.graded() {
                vec![round(s.ndcg), round(s.precision)]
            } else {
                vec![round(s.rr)]
            };
            (c.query.id.clone(), values)
        })
        .collect()
}

/// How one metric's mean moved against the baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Fell,
    Same,
    Rose,
    /// The baseline has no value for the metric.
    Missing,
}

/// A drop past the tolerance falls; a drop of exactly the tolerance does
/// not. Both sides are rounded as the baseline is, so float noise never
/// decides.
pub(crate) fn change(was: Option<&Gate>, now: f64) -> Change {
    let Some(was) = was else { return Change::Missing };
    let delta = round(now - was.mean);
    if delta < -was.tolerance {
        Change::Fell
    } else if delta > was.tolerance {
        Change::Rose
    } else {
        Change::Same
    }
}

/// How far one graded query's nDCG@10 may fall against its baseline value
/// before the gate fails, however the means move.
const QUERY_DROP: f64 = 0.1;

/// Why one query fails the gate on its own, if it does: a graded query whose
/// nDCG@10 fell by more than `QUERY_DROP`, or a known item that was in the
/// top 10 and left it. `was` is the query's baseline values.
pub(crate) fn slipped(
    graded: bool,
    was: &[f64],
    now: &Score,
) -> Option<String> {
    let Some(&first) = was.first() else {
        return Some("has no baseline value".to_owned());
    };
    if graded {
        (round(first - now.ndcg) > QUERY_DROP).then(|| {
            format!("nDCG@10 fell from {first:.3} to {:.3}", now.ndcg)
        })
    } else {
        let was_found = first > 0.0 && (1.0 / first).round() <= real(DEPTH);
        (was_found && !now.found).then(|| {
            format!(
                "the known item left the top 10 (RR {first:.3} to {:.3})",
                now.rr
            )
        })
    }
}

fn check(queries: &[Query], bless: bool) -> Result<()> {
    let cases = cases(queries)?;
    let rankings: Vec<Vec<Hit>> = cases.iter().map(shipped).collect();
    let scores: Vec<Score> =
        cases.iter().zip(&rankings).map(|(c, r)| score(c, r)).collect();
    let mut out = io::stdout().lock();

    let unjudged: usize = scores.iter().map(|s| s.unjudged.len()).sum();
    let pending = cases
        .iter()
        .flat_map(|c| &c.judged.grades)
        .filter(|g| g.is_none())
        .count();
    if unjudged > 0 || pending > 0 {
        for (case, s) in cases.iter().zip(&scores) {
            for d in &s.unjudged {
                writeln!(out, "unjudged  {:<26} {}", case.query.id, d.url)?;
            }
        }
        bail!(
            "{unjudged} top-10 results have no judgment and {pending} labels are still `?`\n  Try:   `just relevance pool`, grade the worksheets, then `just relevance absorb`"
        );
    }

    writeln!(out, "Shipped ranking, {} queries:\n", cases.len())?;
    report::summary(&mut out, &cases, &scores)?;
    report::stability(&mut out, &cases, &scores)?;
    report::empty(&mut out, &cases, &rankings)?;
    writeln!(out)?;
    report::unanswered(&mut out, &cases)?;
    report::pollution(&mut out, &cases, &[("shipped", &rankings)])?;

    let current = Baseline {
        metrics: gates(&cases, &scores),
        queries: per_query(&cases, &scores),
    };
    if bless {
        return write_baseline(&mut out, &current);
    }
    let Some(baseline) = baseline()? else {
        bail!(
            "there is no baseline to compare with\n  Try:   just relevance --bless"
        );
    };
    let failures = gate(&mut out, &cases, &scores, &current, &baseline)?;
    if !failures.is_empty() {
        report::moved(&mut out, &cases, &scores, &baseline)?;
        bail!(
            "the ranking fell against the baseline:\n  {}\n  Try:   if the trade-off is deliberate, `just relevance --bless` and say why in the commit",
            failures.join("\n  ")
        );
    }
    Ok(())
}

/// Prints each metric against the baseline and returns every reason the
/// gate fails: a mean that fell past its tolerance or is missing from the
/// baseline, a query set that differs from the baseline's, and any one
/// query that slipped on its own.
fn gate(
    out: &mut impl Write,
    cases: &[Case],
    scores: &[Score],
    current: &Baseline,
    baseline: &Baseline,
) -> Result<Vec<String>> {
    let mut failures: Vec<String> =
        renamed(current, baseline).into_iter().collect();
    let rose = means(out, current, baseline, &mut failures)?;
    for (case, score) in cases.iter().zip(scores) {
        let Some(was) = baseline.queries.get(&case.query.id) else {
            continue;
        };
        if let Some(why) = slipped(case.query.graded(), was, score) {
            failures.push(format!("{}: {why}", case.query.id));
        }
    }
    if rose && failures.is_empty() {
        writeln!(
            out,
            "\nThe ranking improved past the tolerance. Run `just relevance --bless` so a later drop is measured from here, and say why in the commit."
        )?;
    }
    Ok(failures)
}

/// The queries added and removed since the baseline was written, if any.
fn renamed(current: &Baseline, baseline: &Baseline) -> Option<String> {
    let now: BTreeSet<&str> =
        current.queries.keys().map(String::as_str).collect();
    let was: BTreeSet<&str> =
        baseline.queries.keys().map(String::as_str).collect();
    let join = |ids: Vec<&&str>| {
        ids.into_iter().copied().collect::<Vec<_>>().join(", ")
    };
    (now != was).then(|| {
        format!(
            "the queries differ from the baseline's (new: {}; gone: {})",
            join(now.difference(&was).collect()),
            join(was.difference(&now).collect())
        )
    })
}

/// Prints each metric's mean against the baseline, adds a failure for each
/// that fell or is missing, and says whether any rose past its tolerance.
fn means(
    out: &mut impl Write,
    current: &Baseline,
    baseline: &Baseline,
    failures: &mut Vec<String>,
) -> Result<bool> {
    writeln!(out, "\nAgainst the baseline:")?;
    let mut rose = false;
    for (metric, now) in &current.metrics {
        let was = baseline.metrics.get(metric);
        let verdict = match change(was, now.mean) {
            Change::Fell => {
                failures.push(format!(
                    "{} fell beyond the tolerance",
                    metric.label()
                ));
                "REGRESSION"
            }
            Change::Missing => {
                failures.push(format!(
                    "{} is missing from the baseline",
                    metric.label()
                ));
                "MISSING"
            }
            Change::Rose => {
                rose = true;
                "better"
            }
            Change::Same => "same",
        };
        let Some(was) = was else {
            writeln!(
                out,
                "  {:<15} none -> {:.4}  {verdict}",
                metric.label(),
                now.mean
            )?;
            continue;
        };
        writeln!(
            out,
            "  {:<15} {:.4} -> {:.4}  ({:+.4}, tolerance {:.4})  {verdict}",
            metric.label(),
            was.mean,
            now.mean,
            now.mean - was.mean,
            was.tolerance
        )?;
    }
    Ok(rose)
}

/// Writes `current` as the new baseline, after printing the old means
/// against the new ones.
fn write_baseline(out: &mut impl Write, current: &Baseline) -> Result<()> {
    writeln!(out, "\nBaseline, old -> new:")?;
    let old = match baseline() {
        Ok(old) => old,
        Err(e) => {
            writeln!(out, "  the old baseline is unreadable ({e:#})")?;
            None
        }
    };
    for metric in Metric::ALL {
        let was = old
            .as_ref()
            .and_then(|b| b.metrics.get(&metric))
            .map_or_else(|| "none".to_owned(), |g| format!("{:.4}", g.mean));
        let now = current
            .metrics
            .get(&metric)
            .map_or_else(|| "none".to_owned(), |g| format!("{:.4}", g.mean));
        writeln!(out, "  {:<15} {was} -> {now}", metric.label())?;
    }
    let text = serde_json::to_string_pretty(current)?;
    fs::write(baseline_path(), text + "\n")?;
    writeln!(out, "\nWrote {}.", baseline_path().display())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The production variant reimplements `weigh(merge(..))`; on every
    /// recorded query the two must order the results identically, or no
    /// comparison between variants means anything.
    #[test]
    fn the_production_variant_reproduces_the_shipped_ranking() {
        let queries = snapshot::queries().unwrap();
        let cases = cases(&queries).unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            let urls =
                |hits: Vec<Hit>| hits.into_iter().map(|h| h.dataset.url);
            let shipped: Vec<String> = urls(shipped(&case)).collect();
            let variant: Vec<String> =
                urls(variant(&case, &Config::production(), &Priors::new()))
                    .collect();
            assert_eq!(shipped, variant, "{}", case.query.id);
        }
    }

    fn gate(mean: f64, tolerance: f64) -> Gate {
        Gate { mean, low: mean, high: mean, tolerance }
    }

    #[test]
    fn a_drop_past_the_tolerance_falls_and_one_at_it_does_not() {
        let was = gate(0.6, 0.01);
        assert_eq!(change(Some(&was), 0.589_999), Change::Fell);
        assert_eq!(change(Some(&was), 0.59), Change::Same);
        assert_eq!(change(Some(&was), 0.6), Change::Same);
        assert_eq!(change(Some(&was), 0.62), Change::Rose);
        assert_eq!(change(None, 0.6), Change::Missing);
    }

    #[test]
    fn one_query_slipping_fails_whatever_the_means_do() {
        let graded = |ndcg| Score { ndcg, precision: 1.0, ..Score::default() };
        assert!(slipped(true, &[0.9, 1.0], &graded(0.6)).is_some());
        assert!(slipped(true, &[0.9, 1.0], &graded(0.8)).is_none());
        assert!(slipped(true, &[], &graded(0.8)).is_some());
        let known = |rr, found| Score { rr, found, ..Score::default() };
        assert!(slipped(false, &[0.1], &known(0.0, false)).is_some());
        assert!(slipped(false, &[1.0], &known(0.5, true)).is_none());
        assert!(slipped(false, &[0.0], &known(0.0, false)).is_none());
        assert!(slipped(false, &[1.0 / 11.0], &known(0.0, false)).is_none());
    }

    #[test]
    fn a_malformed_baseline_is_an_error_and_only_a_missing_one_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        assert!(baseline_at(&path).unwrap().is_none());
        fs::write(&path, "{").unwrap();
        let err = format!("{:#}", baseline_at(&path).err().unwrap());
        assert!(err.contains("baseline.json"), "{err}");
    }
}
