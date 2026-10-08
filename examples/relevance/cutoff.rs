//! What it costs to stop waiting for the slowest sources: replays the
//! recorded per-source lists with the sources a stopping rule would have
//! left behind removed, using per-source latencies measured live, and scores
//! each rule's top 10 against the full one and against the judgments.
//!
//! A catalog source counts as answering at once, as it does from the cache
//! (ADR 0018): a time recorded while one downloaded says nothing about the
//! rule.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use dataseek::internals::{Hit, SOURCES, merge, quorum, weigh};
use serde::Deserialize;

use crate::metrics::{DEPTH, Interval, SEED, mean, paired, real};
use crate::snapshot::{Half, Query};
use crate::{Case, Score, cases, score};

/// The default `--timeout`, which bounds every rule.
const DEADLINE_MS: u64 = 20_000;

/// One live search: how long each source took, from `search --json`.
#[derive(Deserialize)]
struct Search {
    query: String,
    sources: Option<Vec<Timed>>,
}

#[derive(Deserialize)]
struct Timed {
    id: String,
    status: String,
    ms: u64,
}

impl Timed {
    fn asked(&self) -> bool {
        !self.status.starts_with("needs $")
            && !self.status.starts_with("skipped, outage")
    }

    /// Whether the source returned a list, as `Status::answered` decides.
    fn answered(&self) -> bool {
        self.status == "ok"
            || self.status == "cached"
            || self.status == "expired catalog"
            || self.status.starts_with("stale cache")
    }

    fn ms(&self) -> u64 {
        let catalog =
            SOURCES.iter().any(|s| s.id == self.id && s.is_catalog());
        if catalog { 0 } else { self.ms }
    }
}

/// One asked source finishing: when, and whether it answered.
#[derive(Clone, Copy)]
struct Arrival {
    ms: u64,
    answered: bool,
}

/// When a search stops waiting for the sources still working.
#[derive(Clone, Copy)]
enum Rule {
    All,
    /// A fixed deadline in milliseconds.
    Cap(u64),
    /// Once this percentage of the sources that have not failed has
    /// answered, this many more milliseconds: `search.rs`'s rule, through
    /// its own `quorum`.
    Quorum(usize, u64),
}

impl Rule {
    fn name(self) -> String {
        match self {
            Self::All => "wait for all".to_owned(),
            Self::Cap(ms) => format!("deadline {ms} ms"),
            Self::Quorum(percent, grace) => format!("{percent}% + {grace} ms"),
        }
    }

    /// The moment the search stops waiting, given every asked source's
    /// arrival in time order.
    fn stop(self, arrivals: &[Arrival]) -> u64 {
        let last = arrivals.last().map_or(0, |a| a.ms);
        let stop = match self {
            Self::All => last,
            Self::Cap(ms) => ms,
            Self::Quorum(percent, grace) => reached(arrivals, percent)
                .map_or(last, |at| at.saturating_add(grace)),
        };
        stop.min(last).min(DEADLINE_MS)
    }
}

/// When the quorum of `percent` is reached, counted as the search loop
/// counts it: answers out of the sources that have not failed.
fn reached(arrivals: &[Arrival], percent: usize) -> Option<u64> {
    let (mut answered, mut failed): (usize, usize) = (0, 0);
    arrivals.iter().find_map(|a| {
        if a.answered {
            answered = answered.saturating_add(1);
        } else {
            failed = failed.saturating_add(1);
        }
        let reachable = arrivals.len().saturating_sub(failed);
        (answered > 0 && answered >= quorum(reachable, percent))
            .then_some(a.ms)
    })
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a share in [0, 1] of a count, rounded up"
)]
fn rank(share: f64, count: usize) -> usize {
    (share * real(count)).ceil() as usize
}

const RULES: [Rule; 13] = [
    Rule::All,
    Rule::Cap(1000),
    Rule::Cap(2000),
    Rule::Cap(3000),
    Rule::Quorum(80, 0),
    Rule::Quorum(80, 1000),
    Rule::Quorum(90, 0),
    Rule::Quorum(90, 500),
    Rule::Quorum(90, 1000),
    Rule::Quorum(90, 2000),
    Rule::Quorum(95, 500),
    Rule::Quorum(95, 1000),
    Rule::Quorum(95, 2000),
];

/// One rule over every recorded search, averaged per query.
struct Outcome {
    rule: Rule,
    /// Per search, in milliseconds.
    waits: Vec<u64>,
    /// Per search, how many answering sources were left behind.
    dropped: Vec<usize>,
    /// Per query, the share of the full top 10 still in this top 10.
    kept: Vec<f64>,
    /// Per query, the rule's score averaged over its searches.
    scores: Vec<Score>,
}

pub fn run(queries: &[Query], latencies: &Path) -> Result<()> {
    let cases = cases(queries)?;
    let searches = load(latencies)?;
    let by_query: HashMap<&str, Vec<&Vec<Timed>>> =
        searches.iter().fold(HashMap::new(), |mut map, s| {
            if let Some(sources) = &s.sources {
                map.entry(s.query.as_str()).or_default().push(sources);
            }
            map
        });
    if let Some(case) =
        cases.iter().find(|c| !by_query.contains_key(c.query.id.as_str()))
    {
        bail!("{} has no search for {}", latencies.display(), case.query.id);
    }
    let full: Vec<Vec<Hit>> = cases.iter().map(crate::shipped).collect();
    let outcomes: Vec<Outcome> = RULES
        .iter()
        .map(|&rule| replay(rule, &cases, &full, &by_query))
        .collect();
    let Some(all) = outcomes.first() else { return Ok(()) };
    let rounds = searches.len();
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "{rounds} searches over {} queries; deltas against waiting for every \
         source, with paired 95% intervals over queries\n",
        cases.len()
    )?;
    writeln!(
        out,
        "{:<16} {:>7} {:>7} {:>7} {:>7} {:>7}  {:<24} {:<24} {:<24}",
        "rule",
        "p50 s",
        "p90 s",
        "max s",
        "left",
        "top10",
        "nDCG@10 tune",
        "nDCG@10 held",
        "known-item MRR"
    )?;
    for o in &outcomes {
        writeln!(
            out,
            "{:<16} {:>7.2} {:>7.2} {:>7.2} {:>7.2} {:>6.1}%  {:<24} {:<24} {:<24}",
            o.rule.name(),
            seconds(percentile(&o.waits, 0.5)),
            seconds(percentile(&o.waits, 0.9)),
            seconds(percentile(&o.waits, 1.0)),
            mean(&o.dropped.iter().map(|&d| real(d)).collect::<Vec<_>>()),
            mean(&o.kept) * 100.0,
            delta(&cases, all, o, Some(Half::Tune), |s| s.ndcg),
            delta(&cases, all, o, Some(Half::Held), |s| s.ndcg),
            delta(&cases, all, o, None, |s| s.rr),
        )?;
    }
    writeln!(
        out,
        "\nleft: answering sources left out per search. top10: share of the \
         full top 10 still shown."
    )?;
    Ok(())
}

fn load(path: &Path) -> Result<Vec<Search>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str(line).with_context(|| {
                format!("{} line {}", path.display(), i.saturating_add(1))
            })
        })
        .collect()
}

fn replay(
    rule: Rule,
    cases: &[Case],
    full: &[Vec<Hit>],
    by_query: &HashMap<&str, Vec<&Vec<Timed>>>,
) -> Outcome {
    let mut outcome = Outcome {
        rule,
        waits: Vec::new(),
        dropped: Vec::new(),
        kept: Vec::new(),
        scores: Vec::new(),
    };
    for (case, full) in cases.iter().zip(full) {
        let searches = by_query
            .get(case.query.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut kept = Vec::new();
        let mut scores = Vec::new();
        for timed in searches {
            let mut arrivals: Vec<Arrival> = timed
                .iter()
                .filter(|t| t.asked())
                .map(|t| Arrival { ms: t.ms(), answered: t.answered() })
                .collect();
            arrivals.sort_by_key(|a| a.ms);
            let stop = rule.stop(&arrivals);
            let late: HashSet<&str> = timed
                .iter()
                .filter(|t| t.asked() && t.ms() > stop)
                .map(|t| t.id.as_str())
                .collect();
            let lists: Vec<_> = case
                .lists
                .iter()
                .map(|(id, list)| {
                    (
                        *id,
                        if late.contains(id) {
                            Vec::new()
                        } else {
                            list.clone()
                        },
                    )
                })
                .collect();
            let left = case
                .lists
                .iter()
                .filter(|(id, list)| late.contains(id) && !list.is_empty())
                .count();
            let ranked = weigh(merge(lists), &case.query.text);
            kept.push(overlap(full, &ranked));
            scores.push(score(case, &ranked));
            outcome.waits.push(stop);
            outcome.dropped.push(left);
        }
        outcome.kept.push(mean(&kept));
        outcome.scores.push(Score {
            ndcg: mean(&scores.iter().map(|s| s.ndcg).collect::<Vec<_>>()),
            precision: mean(
                &scores.iter().map(|s| s.precision).collect::<Vec<_>>(),
            ),
            rr: mean(&scores.iter().map(|s| s.rr).collect::<Vec<_>>()),
            ..Score::default()
        });
    }
    outcome
}

/// The share of `full`'s top 10 links that `ranked`'s top 10 still shows.
fn overlap(full: &[Hit], ranked: &[Hit]) -> f64 {
    let shown: HashSet<&str> =
        ranked.iter().take(DEPTH).map(|h| h.dataset.url.as_str()).collect();
    let wanted: Vec<&str> =
        full.iter().take(DEPTH).map(|h| h.dataset.url.as_str()).collect();
    if wanted.is_empty() {
        return 1.0;
    }
    let found = wanted.iter().filter(|u| shown.contains(*u)).count();
    real(found) / real(wanted.len())
}

/// The paired change of one per-query value against waiting for all, over
/// the graded queries (or, for MRR, the known items) of one half or both.
fn delta(
    cases: &[Case],
    all: &Outcome,
    rule: &Outcome,
    half: Option<Half>,
    value: fn(&Score) -> f64,
) -> String {
    let mrr = half.is_none();
    let (before, after): (Vec<f64>, Vec<f64>) = cases
        .iter()
        .zip(all.scores.iter().zip(&rule.scores))
        .filter(|(c, _)| half.is_none_or(|h| c.query.half == h))
        .filter(|(c, _)| c.query.graded() != mrr)
        .map(|(_, (a, r))| (value(a), value(r)))
        .unzip();
    let Interval { mean, low, high, .. } = paired(&before, &after, SEED);
    format!("{mean:+.3} [{low:+.3}, {high:+.3}]")
}

fn percentile(values: &[u64], q: f64) -> u64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(rank(q, sorted.len()).saturating_sub(1)).copied().unwrap_or(0)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "milliseconds of a search, far below 2^52"
)]
fn seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrivals(each: &[(u64, bool)]) -> Vec<Arrival> {
        each.iter().map(|&(ms, answered)| Arrival { ms, answered }).collect()
    }

    #[test]
    fn a_quorum_stops_a_grace_after_enough_answers() {
        let mut ten: Vec<(u64, bool)> =
            (1..=9).map(|i| (i * 100, true)).collect();
        ten.push((9000, true));
        let ten = arrivals(&ten);
        assert_eq!(Rule::Quorum(90, 1000).stop(&ten), 1900);
        assert_eq!(Rule::Quorum(80, 0).stop(&ten), 800);
        assert_eq!(Rule::All.stop(&ten), 9000);
        assert_eq!(Rule::Cap(2000).stop(&ten), 2000);
    }

    #[test]
    fn fast_failures_do_not_make_a_quorum() {
        let mut ten: Vec<(u64, bool)> =
            (1..=9).map(|i| (i * 10, false)).collect();
        ten.push((3000, true));
        assert_eq!(Rule::Quorum(90, 1000).stop(&arrivals(&ten)), 3000);
    }

    #[test]
    fn no_rule_waits_past_the_last_source_or_the_deadline() {
        let short = arrivals(&[(100, true), (200, true)]);
        assert_eq!(Rule::Cap(3000).stop(&short), 200);
        let slow = arrivals(&[(30_000, true)]);
        assert_eq!(Rule::All.stop(&slow), DEADLINE_MS);
    }
}
