//! Ranking variants: every way of ordering the merged hits that the
//! benchmark compares against the shipped one. Each is computed from the
//! hits `merge` returns (identity merging is never varied) and their
//! per-source ranks, so a variant differs from production only in scoring.

use std::collections::HashMap;

use dataseek::internals::{Hit, merge, needles, terms, words};

use crate::metrics::real;
use crate::snapshot::Lists;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fusion {
    /// Reciprocal rank fusion: the sum over sources of `1 / (k + rank)`,
    /// rank counted from 1 (Cormack, Clarke and Buettcher, SIGIR 2009).
    Rrf(f64),
    /// RRF times the number of sources that found the hit, the CombMNZ
    /// form (Fox and Shaw, TREC-2, 1994).
    Mnz(f64),
    /// Borda count normalized by list length: `(n - rank) / n` per source
    /// (Aslam and Montague, SIGIR 2001).
    Borda,
    /// Each source's first, then each source's second, and so on: the
    /// ordering fusion degenerates to when lists never overlap.
    RoundRobin,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Match {
    /// Share of query terms in the title or description.
    Coverage,
    /// A term in the title counts 1, only in the description 1/2.
    TitleWeighted,
    /// Terms weighted by inverse document frequency over the query's own
    /// merged candidates, so a term every candidate carries counts little.
    Idf,
    /// No rescoring at all: plain fusion.
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Thin {
    Keep,
    /// Records with neither description nor publisher scaled by this.
    Scale(f64),
    /// Records without a description dropped when no query term is in the
    /// title, as described ones already are.
    DropUnmatched,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// How many of each source's results fusion reads.
    pub depth: usize,
    pub fusion: Fusion,
    /// Per-source weights from the tuning half's labels.
    pub priors: bool,
    pub matching: Match,
    /// Ignore function words that natural-language queries carry.
    pub stopwords: bool,
    /// Score multiplier `floor + (1 - floor) * coverage^power`.
    pub floor: f64,
    pub power: f64,
    /// Drop a described hit that mentions no query term.
    pub drop_unmatched: bool,
    /// Multiplier when the whole query appears as a phrase in the title.
    pub phrase: f64,
    pub thin: Thin,
}

impl Config {
    /// What `weigh(merge(..))` does today, written as a variant; a test in
    /// main.rs holds the two to the same order on every query.
    pub const fn production() -> Self {
        Self {
            depth: 10,
            fusion: Fusion::Rrf(60.0),
            priors: false,
            matching: Match::Idf,
            stopwords: false,
            floor: 0.25,
            power: 1.0,
            drop_unmatched: true,
            phrase: 1.0,
            thin: Thin::Keep,
        }
    }
}

/// One setting of one family, applied on top of another configuration.
pub struct Setting {
    pub family: &'static str,
    pub name: String,
    pub apply: fn(&mut Config, f64),
    pub value: f64,
}

impl Setting {
    pub fn on(&self, base: &Config) -> Config {
        let mut config = *base;
        (self.apply)(&mut config, self.value);
        config
    }
}

fn setting(
    family: &'static str,
    name: &str,
    value: f64,
    apply: fn(&mut Config, f64),
) -> Setting {
    Setting { family, name: name.to_owned(), apply, value }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "depths are small whole numbers written in this file"
)]
fn depth(value: f64) -> usize {
    value as usize
}

/// Every setting the benchmark tries, by family. A family's settings are
/// alternatives; settings from different families can stack.
pub fn settings() -> Vec<Setting> {
    let mut all = Vec::new();
    for k in [1.0, 10.0, 20.0, 40.0, 100.0, 200.0] {
        all.push(setting("rrf-k", &format!("k = {k}"), k, |c, v| {
            c.fusion = match c.fusion {
                Fusion::Mnz(_) => Fusion::Mnz(v),
                _ => Fusion::Rrf(v),
            };
        }));
    }
    for n in [3.0, 5.0] {
        all.push(setting(
            "depth",
            &format!("top {n} per source"),
            n,
            |c, v| {
                c.depth = depth(v);
            },
        ));
    }
    all.push(setting("fusion", "CombMNZ", 0.0, |c, _| {
        c.fusion = match c.fusion {
            Fusion::Rrf(k) | Fusion::Mnz(k) => Fusion::Mnz(k),
            _ => Fusion::Mnz(60.0),
        };
    }));
    all.push(setting("fusion", "Borda", 0.0, |c, _| c.fusion = Fusion::Borda));
    all.push(setting("fusion", "round robin", 0.0, |c, _| {
        c.fusion = Fusion::RoundRobin;
    }));
    all.push(setting("priors", "source priors", 0.0, |c, _| c.priors = true));
    for floor in [0.0, 0.1, 0.5] {
        all.push(setting(
            "floor",
            &format!("floor {floor}"),
            floor,
            |c, v| {
                c.floor = v;
            },
        ));
    }
    all.push(setting("power", "coverage squared", 2.0, |c, v| c.power = v));
    all.push(setting("match", "title-weighted", 0.0, |c, _| {
        c.matching = Match::TitleWeighted;
    }));
    all.push(setting("match", "plain term count", 0.0, |c, _| {
        c.matching = Match::Coverage;
    }));
    all.push(setting("match", "no rescoring", 0.0, |c, _| {
        c.matching = Match::Off;
        c.drop_unmatched = false;
    }));
    all.push(setting("drop", "keep unmatched", 0.0, |c, _| {
        c.drop_unmatched = false;
    }));
    all.push(setting("stopwords", "ignore function words", 0.0, |c, _| {
        c.stopwords = true;
    }));
    for bonus in [1.5, 2.0] {
        all.push(setting(
            "phrase",
            &format!("phrase x{bonus}"),
            bonus,
            |c, v| {
                c.phrase = v;
            },
        ));
    }
    for scale in [0.5, 0.75] {
        all.push(setting("thin", &format!("thin x{scale}"), scale, |c, v| {
            c.thin = Thin::Scale(v);
        }));
    }
    all.push(setting("thin", "drop unmatched thin", 0.0, |c, _| {
        c.thin = Thin::DropUnmatched;
    }));
    all
}

/// Words a natural-language query carries that say nothing about the data:
/// question words, prepositions, quantifiers. The catalog stopwords are
/// filtered already.
const FUNCTION_WORDS: [&str; 30] = [
    "about", "across", "after", "against", "all", "are", "at", "be", "before",
    "between", "by", "do", "does", "each", "from", "has", "have", "how", "is",
    "many", "much", "or", "over", "per", "since", "that", "what", "where",
    "which", "who",
];

fn query_terms(query: &str, stopwords: bool) -> Vec<String> {
    let all = terms(query);
    if !stopwords {
        return all;
    }
    let kept: Vec<String> = all
        .iter()
        .filter(|t| !FUNCTION_WORDS.contains(&t.as_str()))
        .cloned()
        .collect();
    if kept.is_empty() { all } else { kept }
}

/// Per-source weights: the mean gain (grade / 2) of a source's labelled
/// results, shrunk toward the mean of all sources with the weight of
/// `PRIOR_STRENGTH` results, as a ratio to that mean. A source with few
/// labels stays near 1.
pub type Priors = HashMap<&'static str, f64>;

pub const PRIOR_STRENGTH: f64 = 10.0;

pub fn priors(evidence: &[(&'static str, f64)]) -> Priors {
    let overall = crate::metrics::mean(
        &evidence.iter().map(|(_, g)| *g).collect::<Vec<_>>(),
    );
    if overall <= 0.0 {
        return Priors::new();
    }
    let mut sums: HashMap<&'static str, (f64, f64)> = HashMap::new();
    for (source, gain) in evidence {
        let entry = sums.entry(source).or_insert((0.0, 0.0));
        entry.0 += gain;
        entry.1 += 1.0;
    }
    sums.into_iter()
        .map(|(source, (total, n))| {
            let shrunk =
                (total + PRIOR_STRENGTH * overall) / (n + PRIOR_STRENGTH);
            (source, shrunk / overall)
        })
        .collect()
}

/// The merged hits a configuration ranks: `merge` over each list cut to
/// the configured depth.
pub fn candidates(lists: &Lists, config: &Config) -> Vec<Hit> {
    if config.depth >= 10 {
        return merge(lists.clone());
    }
    let cut: Lists = lists
        .iter()
        .map(|(s, ds)| (*s, ds.iter().take(config.depth).cloned().collect()))
        .collect();
    merge(cut)
}

/// The hits in `config`'s order, as indices into `hits`, with the dropped
/// ones left out.
pub fn rank(
    hits: &[Hit],
    lists: &Lists,
    query: &str,
    config: &Config,
    priors: &Priors,
) -> Vec<usize> {
    let lengths: HashMap<&str, usize> =
        lists.iter().map(|(s, ds)| (*s, ds.len().min(config.depth))).collect();
    let query_terms = query_terms(query, config.stopwords);
    let needles = needles(&query_terms);
    let phrase = format!(" {}", query_terms.join(" "));
    let texts: Vec<(String, String)> = hits
        .iter()
        .map(|h| {
            (
                words(&h.dataset.title),
                words(h.dataset.description.as_deref().unwrap_or("")),
            )
        })
        .collect();
    let idf: Vec<f64> = needles
        .iter()
        .map(|n| {
            let df = texts
                .iter()
                .filter(|(t, d)| {
                    t.contains(n.as_str()) || d.contains(n.as_str())
                })
                .count();
            let (n_docs, df) = (real(hits.len()), real(df));
            ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln()
        })
        .collect();

    let mut scored: Vec<(f64, usize, usize)> = hits
        .iter()
        .zip(&texts)
        .enumerate()
        .filter_map(|(i, (hit, (title, body)))| {
            let fused =
                fuse(hit, config.fusion, &lengths, priors, config.priors);
            if query_terms.is_empty() || config.matching == Match::Off {
                return Some((fused, hit.sources.len(), i));
            }
            let in_title: Vec<bool> =
                needles.iter().map(|n| title.contains(n.as_str())).collect();
            let in_body: Vec<bool> =
                needles.iter().map(|n| body.contains(n.as_str())).collect();
            let found = in_title
                .iter()
                .zip(&in_body)
                .filter(|(t, b)| **t || **b)
                .count();
            let described = hit.dataset.description.is_some();
            if found == 0 && config.drop_unmatched && described {
                return None;
            }
            let thin = !described && hit.dataset.publisher.is_none();
            if found == 0 && thin && config.thin == Thin::DropUnmatched {
                return None;
            }
            let coverage =
                coverage(config.matching, &needles, &in_title, &in_body, &idf);
            let mut score = fused
                * (config.floor
                    + (1.0 - config.floor) * coverage.powf(config.power));
            if config.phrase > 1.0 && title.contains(&phrase) {
                score *= config.phrase;
            }
            if let Thin::Scale(scale) = config.thin
                && thin
            {
                score *= scale;
            }
            Some((score, hit.sources.len(), i))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    scored.into_iter().map(|(_, _, i)| i).collect()
}

/// The share of the query a hit covers, as `matching` measures it.
fn coverage(
    matching: Match,
    needles: &[String],
    in_title: &[bool],
    in_body: &[bool],
    idf: &[f64],
) -> f64 {
    let both = in_title.iter().zip(in_body);
    match matching {
        Match::Coverage => {
            real(both.filter(|(t, b)| **t || **b).count())
                / real(needles.len())
        }
        Match::TitleWeighted => {
            let points: f64 = both
                .map(|(t, b)| match (t, b) {
                    (true, _) => 1.0,
                    (false, true) => 0.5,
                    (false, false) => 0.0,
                })
                .sum();
            points / real(needles.len())
        }
        Match::Idf => {
            let total: f64 = idf.iter().sum();
            let got: f64 = idf
                .iter()
                .zip(both)
                .filter(|(_, (t, b))| **t || **b)
                .map(|(w, _)| w)
                .sum();
            if total > 0.0 { got / total } else { 1.0 }
        }
        Match::Off => 1.0,
    }
}

fn fuse(
    hit: &Hit,
    fusion: Fusion,
    lengths: &HashMap<&str, usize>,
    priors: &Priors,
    use_priors: bool,
) -> f64 {
    let weight = |source: &str| {
        if use_priors {
            priors.get(source).copied().unwrap_or(1.0)
        } else {
            1.0
        }
    };
    let pairs = hit.sources.iter().zip(&hit.ranks);
    match fusion {
        Fusion::Rrf(k) => {
            pairs.map(|(s, r)| weight(s) / (k + real(*r) + 1.0)).sum()
        }
        Fusion::Mnz(k) => {
            let sum: f64 =
                pairs.map(|(s, r)| weight(s) / (k + real(*r) + 1.0)).sum();
            sum * real(hit.sources.len())
        }
        Fusion::Borda => pairs
            .map(|(s, r)| {
                let n = real(lengths.get(s).copied().unwrap_or(1).max(1));
                weight(s) * (n - real(*r)) / n
            })
            .sum(),
        Fusion::RoundRobin => {
            let best = hit.ranks.iter().min().copied().unwrap_or(0);
            1.0 / (real(best) + 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use dataseek::internals::Dataset;

    use super::*;

    fn record(title: &str, url: &str, description: Option<&str>) -> Dataset {
        Dataset::new(title, url).describe(description.map(str::to_owned))
    }

    #[test]
    fn idf_weighting_favors_the_rarer_query_word() {
        // "inflation" is in one candidate, "country" in all three, so the
        // record matching only "inflation" outranks the two that match only
        // "country"; plain coverage would tie all three.
        let lists: Lists = vec![(
            "s",
            vec![
                record("Country codes", "https://x.org/1", Some("by country")),
                record("Country borders", "https://x.org/2", Some("country")),
                record("Inflation", "https://x.org/3", Some("prices")),
            ],
        )];
        let hits = merge(lists.clone());
        let config = Config { matching: Match::Idf, ..Config::production() };
        let order =
            rank(&hits, &lists, "inflation country", &config, &Priors::new());
        assert_eq!(hits[order[0]].dataset.title, "Inflation");
    }

    #[test]
    fn function_words_are_ignored_only_when_asked() {
        assert_eq!(
            query_terms("rainfall since 2000", false),
            ["rainfall", "since", "2000"]
        );
        assert_eq!(
            query_terms("rainfall since 2000", true),
            ["rainfall", "2000"]
        );
        assert_eq!(query_terms("how many", true), ["how", "many"]);
    }

    #[test]
    fn priors_shrink_toward_one_with_few_labels() {
        let mut evidence = vec![("good", 1.0); 30];
        evidence.extend(vec![("bad", 0.0); 30]);
        evidence.push(("rare", 0.0));
        let p = priors(&evidence);
        assert!(p["good"] > 1.5 && p["bad"] < 0.5, "{p:?}");
        assert!(p["rare"] > 0.85, "{p:?}");
    }

    #[test]
    fn borda_scores_a_last_place_in_a_short_list_below_a_middle_one_in_a_long_list()
     {
        let lengths: HashMap<&str, usize> =
            [("short", 2), ("long", 10)].into();
        let hit = |s: &'static str, r: usize| Hit {
            dataset: Dataset::default(),
            sources: vec![s],
            ranks: vec![r],
            score: 0.0,
        };
        let last_short = fuse(
            &hit("short", 1),
            Fusion::Borda,
            &lengths,
            &Priors::new(),
            false,
        );
        let middle_long = fuse(
            &hit("long", 4),
            Fusion::Borda,
            &lengths,
            &Priors::new(),
            false,
        );
        assert!(last_short < middle_long);
    }
}
