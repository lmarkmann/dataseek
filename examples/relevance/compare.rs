//! `variants`: every ranking variant scored on both halves, the tuning-half
//! winners judged on the held-out half, and the labels most worth a second
//! look.

use std::collections::HashSet;
use std::io::{self, Write};

use anyhow::Result;
use dataseek::internals::{Hit, merge};

use crate::metrics::{DEPTH, Grade, Interval, SEED, bootstrap, paired};
use crate::report::{self, show};
use crate::snapshot::{Half, Query};
use crate::variants::{self, Config, Priors, Setting};
use crate::{Case, HELD, Metric, Score, TUNE, cases, graded, score, variant};

pub struct Evaluated {
    pub name: String,
    pub rankings: Vec<Vec<Hit>>,
    pub scores: Vec<Score>,
}

impl Evaluated {
    pub fn new(
        name: String,
        config: Config,
        cases: &[Case],
        priors: &Priors,
    ) -> Self {
        let rankings: Vec<Vec<Hit>> =
            cases.iter().map(|c| variant(c, &config, priors)).collect();
        let scores =
            cases.iter().zip(&rankings).map(|(c, r)| score(c, r)).collect();
        Self { name, rankings, scores }
    }

    fn mean(&self, cases: &[Case], metric: Metric, part: crate::Part) -> f64 {
        metric.mean(cases, &self.scores, part)
    }

    fn top(&self, case: usize) -> &[Hit] {
        self.rankings.get(case).map_or(&[], |r| r.get(..DEPTH).unwrap_or(r))
    }
}

/// Per-source priors from the tuning half only: the gain (grade / 2) of
/// every labelled result each source contributed to a tuning query.
pub fn priors(cases: &[Case]) -> Priors {
    let mut evidence = Vec::new();
    let tuning = cases
        .iter()
        .filter(|c| c.query.graded() && c.query.half == Half::Tune);
    for case in tuning {
        for hit in merge(&case.lists) {
            if let Some(g) = case.judged.grade(&hit.dataset) {
                for source in &hit.sources {
                    evidence.push((*source, f64::from(g) / 2.0));
                }
            }
        }
    }
    variants::priors(&evidence)
}

/// Forward selection on the tuning half: from the shipped configuration,
/// add the setting (from a family not yet used) that raises tuning nDCG@10
/// most, until none raises it. Each step is returned.
fn greedy(
    cases: &[Case],
    settings: &[Setting],
    priors: &Priors,
) -> Vec<(String, Config)> {
    let tuning = |config: &Config| {
        Evaluated::new(String::new(), *config, cases, priors).mean(
            cases,
            Metric::Ndcg,
            TUNE,
        )
    };
    let mut current = Config::production();
    let mut best = tuning(&current);
    let mut used: Vec<&str> = Vec::new();
    let mut path: Vec<(String, Config)> = Vec::new();
    loop {
        let step = settings
            .iter()
            .filter(|s| !used.contains(&s.family))
            .map(|s| {
                let config = s.on(&current);
                (tuning(&config), s, config)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let Some((score, s, config)) = step else { return path };
        if score <= best + 1e-9 {
            return path;
        }
        best = score;
        used.push(s.family);
        current = config;
        let name = path.last().map_or_else(
            || s.name.clone(),
            |(n, _)| format!("{n} + {}", s.name),
        );
        path.push((name, current));
    }
}

/// Every configuration compared: the shipped one, each setting alone, and
/// each step of the greedy stack.
pub fn configs(cases: &[Case], priors: &Priors) -> Vec<(String, Config)> {
    let settings = variants::settings();
    let shipped = Config::production();
    let mut all = vec![("shipped".to_owned(), shipped)];
    for s in &settings {
        all.push((format!("{}: {}", s.family, s.name), s.on(&shipped)));
    }
    for (name, config) in greedy(cases, &settings, priors) {
        all.push((format!("stack: {name}"), config));
    }
    all
}

/// The adoption rule: on the held-out half, the paired 95% interval of the
/// change in nDCG@10 lies above zero, and known-item MRR falls by no more
/// than the gate's tolerance.
struct Verdict {
    shift: Interval,
    mrr: f64,
    adopt: bool,
}

fn verdict(
    cases: &[Case],
    base: &[Score],
    candidate: &[Score],
    mrr_tolerance: f64,
) -> Verdict {
    let shift = paired(
        &Metric::Ndcg.values(cases, base, HELD),
        &Metric::Ndcg.values(cases, candidate, HELD),
        SEED,
    );
    let mrr = Metric::Mrr.mean(cases, candidate, HELD)
        - Metric::Mrr.mean(cases, base, HELD);
    Verdict { shift, mrr, adopt: shift.low > 0.0 && mrr >= -mrr_tolerance }
}

fn interval(i: Interval) -> String {
    format!("{:+.3} [{:+.3}, {:+.3}]", i.mean, i.low, i.high)
}

pub fn run(queries: &[Query]) -> Result<()> {
    let cases = cases(queries)?;
    let priors = priors(&cases);
    let evaluated: Vec<Evaluated> = configs(&cases, &priors)
        .into_iter()
        .map(|(name, config)| Evaluated::new(name, config, &cases, &priors))
        .collect();
    let Some(base) = evaluated.first() else { return Ok(()) };
    let mrr_tolerance = crate::baseline()
        .and_then(|b| b.metrics.get(&Metric::Mrr).map(|g| g.tolerance))
        .unwrap_or(0.0);
    let mut out = io::stdout().lock();

    table(&mut out, &cases, base, &evaluated, mrr_tolerance)?;

    let finalists = finalists(&cases, base, &evaluated);
    writeln!(
        out,
        "\nBest of each family on the tuning half, judged on the held-out half:"
    )?;
    for f in &finalists {
        let v = verdict(&cases, &base.scores, &f.scores, mrr_tolerance);
        writeln!(
            out,
            "  {:<58} nDCG@10 {}  MRR {:+.3}  {}",
            f.name,
            interval(v.shift),
            v.mrr,
            if v.adopt { "ADOPT" } else { "reject" }
        )?;
    }

    for f in finalists.iter().filter(|f| f.name.starts_with("stack: ")) {
        writeln!(out, "\n{}:", f.name)?;
        report::summary(&mut out, &cases, &f.scores)?;
        writeln!(out)?;
        let named =
            [("shipped", base.rankings.as_slice()), ("stack", &f.rankings)];
        report::pollution(&mut out, &cases, &named)?;
    }

    writeln!(out)?;
    flips(&mut out, &cases, base, &finalists, mrr_tolerance)?;
    partly(&mut out, &cases, base, &finalists)?;
    zeros(&mut out, &cases, base)?;
    Ok(())
}

fn table(
    out: &mut impl Write,
    cases: &[Case],
    base: &Evaluated,
    evaluated: &[Evaluated],
    mrr_tolerance: f64,
) -> Result<()> {
    writeln!(
        out,
        "{:<54} {:>6} {:>6} {:>6} {:>6} {:>22} {:>6} {:>6} {:>4}",
        "variant (nDCG@10, P@10, MRR by half)",
        "nDCG t",
        "nDCG h",
        "P t",
        "P h",
        "held nDCG change",
        "MRR t",
        "MRR h",
        "new"
    )?;
    for e in evaluated {
        let v = verdict(cases, &base.scores, &e.scores, mrr_tolerance);
        let unjudged: usize = e.scores.iter().map(|s| s.unjudged.len()).sum();
        writeln!(
            out,
            "{:<54} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>22} {:>6.3} {:>6.3} {:>4}",
            e.name,
            e.mean(cases, Metric::Ndcg, TUNE),
            e.mean(cases, Metric::Ndcg, HELD),
            e.mean(cases, Metric::Precision, TUNE),
            e.mean(cases, Metric::Precision, HELD),
            interval(v.shift),
            e.mean(cases, Metric::Mrr, TUNE),
            e.mean(cases, Metric::Mrr, HELD),
            unjudged
        )?;
    }
    writeln!(
        out,
        "\nshipped, all queries: nDCG@10 {}",
        show(bootstrap(
            &Metric::Ndcg.values(cases, &base.scores, crate::Part::All),
            SEED,
            crate::metrics::RESAMPLES
        ))
    )?;
    Ok(())
}

/// The best setting of each family on the tuning half, where it beats the
/// shipped ranking there, then the full greedy stack.
fn finalists<'a>(
    cases: &[Case],
    base: &Evaluated,
    evaluated: &'a [Evaluated],
) -> Vec<&'a Evaluated> {
    let tuning = |e: &Evaluated| e.mean(cases, Metric::Ndcg, TUNE);
    let mut families: Vec<&str> =
        variants::settings().iter().map(|s| s.family).collect();
    families.dedup();
    let mut chosen: Vec<&Evaluated> = families
        .iter()
        .filter_map(|family| {
            let prefix = format!("{family}: ");
            evaluated
                .iter()
                .filter(|e| e.name.starts_with(&prefix))
                .max_by(|a, b| tuning(a).total_cmp(&tuning(b)))
        })
        .filter(|w| tuning(w) > tuning(base) + 1e-9)
        .collect();
    chosen.extend(
        evaluated.iter().rev().find(|e| e.name.starts_with("stack: ")),
    );
    chosen
}

/// Labels whose flip (0 to 2, 1 or 2 to 0) would change an adoption
/// decision: the ones the decisions rest on.
fn flips(
    out: &mut impl Write,
    cases: &[Case],
    base: &Evaluated,
    finalists: &[&Evaluated],
    mrr_tolerance: f64,
) -> Result<()> {
    writeln!(out, "Labels whose flip would change an adoption decision:")?;
    let mut flagged = 0_usize;
    let held: Vec<usize> = (0..cases.len())
        .filter(|&i| {
            cases.get(i).is_some_and(|c| {
                c.query.graded() && c.query.half == Half::Held
            })
        })
        .collect();
    for f in finalists {
        let decision = verdict(cases, &base.scores, &f.scores, mrr_tolerance);
        for &ci in &held {
            let Some(case) = cases.get(ci) else { continue };
            let mut seen = HashSet::new();
            for hit in base.top(ci).iter().chain(f.top(ci)) {
                let Some(index) = case.judged.index(&hit.dataset) else {
                    continue;
                };
                let Some(g) = case.judged.grade(&hit.dataset) else {
                    continue;
                };
                if !seen.insert(index) {
                    continue;
                }
                let flipped: Grade = if g == 0 { 2 } else { 0 };
                let judged = case.judged.with(index, flipped);
                let rescore = |scores: &[Score], ranking: &Evaluated| {
                    let mut scores = scores.to_vec();
                    if let Some(slot) = scores.get_mut(ci) {
                        *slot = graded(
                            &judged,
                            ranking
                                .rankings
                                .get(ci)
                                .map_or(&[], Vec::as_slice),
                        );
                    }
                    scores
                };
                let after = verdict(
                    cases,
                    &rescore(&base.scores, base),
                    &rescore(&f.scores, f),
                    mrr_tolerance,
                );
                if after.adopt != decision.adopt {
                    flagged = flagged.saturating_add(1);
                    writeln!(
                        out,
                        "  {:<24} grade {g}  {}  ({})",
                        case.query.id, hit.dataset.title, f.name
                    )?;
                }
            }
        }
    }
    if flagged == 0 {
        writeln!(out, "  none")?;
    }
    Ok(())
}

/// Grade-1 labels in a held-out top 10: the borderline calls the held-out
/// scores lean on.
fn partly(
    out: &mut impl Write,
    cases: &[Case],
    base: &Evaluated,
    finalists: &[&Evaluated],
) -> Result<()> {
    writeln!(out, "\nGrade-1 labels in held-out top 10s:")?;
    let mut seen = HashSet::new();
    for (ci, case) in cases.iter().enumerate() {
        if !case.query.graded() || case.query.half != Half::Held {
            continue;
        }
        for e in std::iter::once(base).chain(finalists.iter().copied()) {
            for hit in e.top(ci) {
                if case.judged.grade(&hit.dataset) == Some(1)
                    && seen.insert((ci, hit.dataset.url.clone()))
                {
                    writeln!(
                        out,
                        "  {:<24} {}",
                        case.query.id, hit.dataset.title
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Results the shipped ranking puts in its top 3 that are labelled 0: the
/// harshest calls against it.
fn zeros(
    out: &mut impl Write,
    cases: &[Case],
    base: &Evaluated,
) -> Result<()> {
    writeln!(out, "\nTop-3 results of the shipped ranking labelled 0:")?;
    for (ci, case) in cases.iter().enumerate() {
        if !case.query.graded() {
            continue;
        }
        for (pos, hit) in base.top(ci).iter().take(3).enumerate() {
            if case.judged.grade(&hit.dataset) == Some(0) {
                writeln!(
                    out,
                    "  {:<24} #{} {}",
                    case.query.id,
                    pos.saturating_add(1),
                    hit.dataset.title
                )?;
            }
        }
    }
    Ok(())
}
