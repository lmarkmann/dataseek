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

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Result, bail};
use dataseek::internals::{Dataset, Hit, merge, weigh};
use serde::{Deserialize, Serialize};

use metrics::{DEPTH, Grade, SEED, bootstrap, mean};
use snapshot::{Half, Judged, Lists, Query};
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
    pub(crate) judged: Judged,
    pub(crate) targets: Vec<String>,
}

pub(crate) fn cases(queries: &[Query]) -> Result<Vec<Case>> {
    let labels = snapshot::labels()?;
    queries
        .iter()
        .map(|q| {
            Ok(Case {
                lists: snapshot::load(q)?.lists,
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

pub(crate) fn baseline() -> Option<Baseline> {
    let text = fs::read_to_string(baseline_path()).ok()?;
    serde_json::from_str(&text).ok()
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
    report::pollution(&mut out, &cases, &[("shipped", &rankings)])?;

    let current = gates(&cases, &scores);
    if bless {
        let queries = cases
            .iter()
            .zip(&scores)
            .map(|(c, s)| {
                let values = if c.query.graded() {
                    vec![round(s.ndcg), round(s.precision)]
                } else {
                    vec![round(s.rr)]
                };
                (c.query.id.clone(), values)
            })
            .collect();
        let baseline = Baseline { metrics: current, queries };
        let text = serde_json::to_string_pretty(&baseline)?;
        fs::write(baseline_path(), text + "\n")?;
        writeln!(out, "\nWrote {}.", baseline_path().display())?;
        return Ok(());
    }

    let Some(baseline) = baseline() else {
        bail!(
            "there is no baseline to compare with\n  Try:   just relevance --bless"
        );
    };
    writeln!(out, "\nAgainst the baseline:")?;
    let mut fell = Vec::new();
    for metric in Metric::ALL {
        let (Some(was), Some(now)) =
            (baseline.metrics.get(&metric), current.get(&metric))
        else {
            continue;
        };
        let change = now.mean - was.mean;
        let verdict = if change < -was.tolerance {
            fell.push(metric.label());
            "REGRESSION"
        } else if change > was.tolerance {
            "better"
        } else {
            "same"
        };
        writeln!(
            out,
            "  {:<15} {:.4} -> {:.4}  ({change:+.4}, tolerance {:.4})  {verdict}",
            metric.label(),
            was.mean,
            now.mean,
            was.tolerance
        )?;
    }
    if !fell.is_empty() {
        report::moved(&mut out, &cases, &scores, &baseline)?;
        bail!(
            "{} fell beyond the tolerance\n  Try:   if the trade-off is deliberate, `just relevance --bless` and say why in the commit",
            fell.join(" and ")
        );
    }
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
        for case in cases(&queries).unwrap() {
            let urls =
                |hits: Vec<Hit>| hits.into_iter().map(|h| h.dataset.url);
            let shipped: Vec<String> = urls(shipped(&case)).collect();
            let variant: Vec<String> =
                urls(variant(&case, &Config::production(), &Priors::new()))
                    .collect();
            assert_eq!(shipped, variant, "{}", case.query.id);
        }
    }
}
