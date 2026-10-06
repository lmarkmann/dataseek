//! Merging every source's ranked list into one: duplicates collapse into a
//! single hit that remembers all the sources that found it, and the merged
//! list is ordered by reciprocal rank fusion.
//!
//! Two records are the same dataset when they share any identity key: a DOI
//! (including concept DOIs carried as aliases, which folds DataCite's
//! per-version records), a normalized landing URL, or a normalized title from
//! the same publisher. A title key only ever joins records from different
//! sources: within one source, two records with one name are two datasets
//! (Data Commons variables, DataCite re-uploads). Merging is transitive. Fusion scores a hit as the sum
//! over its sources of `1 / (K + rank)`, so a dataset two sources both rank
//! highly beats one that a single source ranks first; within one source only
//! the best rank counts, so a source cannot outvote others with duplicates.

use std::collections::HashMap;

use serde::Serialize;

use crate::record::{Dataset, doi};

/// The fusion constant from Cormack, Clarke and Buettcher (SIGIR 2009). It
/// damps the gap between rank 1 and rank 2 so no single source dominates.
const K: f64 = 60.0;

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    #[serde(flatten)]
    pub dataset: Dataset,
    pub sources: Vec<&'static str>,
    pub score: f64,
}

/// Fuse per-source result lists, each already in that source's rank order.
pub fn merge(lists: &[(&'static str, Vec<Dataset>)]) -> Vec<Hit> {
    let mut hits: Vec<Option<Building>> = Vec::new();
    let mut owner: HashMap<String, usize> = HashMap::new();

    for (source, datasets) in lists {
        for (rank, dataset) in datasets.iter().enumerate() {
            let keys = identity_keys(dataset);
            let mut targets: Vec<usize> = keys
                .iter()
                .filter_map(|k| {
                    let index = owner.get(k).copied()?;
                    let same_source = hits
                        .get(index)
                        .and_then(Option::as_ref)
                        .is_some_and(|hit| hit.has(source));
                    (!k.starts_with("title:") || !same_source).then_some(index)
                })
                .collect();
            targets.sort_unstable();
            targets.dedup();

            let index = if let Some(&first) = targets.first() {
                for &other in targets.iter().skip(1) {
                    absorb(&mut hits, &mut owner, first, other);
                }
                if let Some(Some(hit)) = hits.get_mut(first) {
                    hit.add(source, rank, dataset, &keys);
                }
                first
            } else {
                hits.push(Some(Building::new(source, rank, dataset, &keys)));
                hits.len().saturating_sub(1)
            };
            for key in keys {
                owner.insert(key, index);
            }
        }
    }

    rank(hits.into_iter().flatten().map(Building::finish).collect())
}

/// Highest score first, more sources breaking a tie, then the input order.
/// Only the keys are sorted, so each hit, a few hundred bytes, moves once.
fn rank(hits: Vec<Hit>) -> Vec<Hit> {
    let mut order: Vec<(f64, usize, usize)> = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| (hit.score, hit.sources.len(), i))
        .collect();
    order.sort_unstable_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let mut slots: Vec<Option<Hit>> = hits.into_iter().map(Some).collect();
    order.iter().filter_map(|&(_, _, i)| slots.get_mut(i)?.take()).collect()
}

struct Building {
    dataset: Dataset,
    best_rank: Vec<(&'static str, usize)>,
    keys: Vec<String>,
}

impl Building {
    fn new(
        source: &'static str,
        rank: usize,
        dataset: &Dataset,
        keys: &[String],
    ) -> Self {
        Self {
            dataset: dataset.clone(),
            best_rank: vec![(source, rank)],
            keys: keys.to_vec(),
        }
    }

    fn has(&self, source: &str) -> bool {
        self.best_rank.iter().any(|(s, _)| *s == source)
    }

    fn add(
        &mut self,
        source: &'static str,
        rank: usize,
        dataset: &Dataset,
        keys: &[String],
    ) {
        match self.best_rank.iter_mut().find(|(s, _)| *s == source) {
            Some((_, best)) => *best = (*best).min(rank),
            None => self.best_rank.push((source, rank)),
        }
        fill(&mut self.dataset, dataset);
        for key in keys {
            if !self.keys.contains(key) {
                self.keys.push(key.clone());
            }
        }
    }

    fn absorb(&mut self, other: Self) {
        for (source, rank) in other.best_rank {
            match self.best_rank.iter_mut().find(|(s, _)| *s == source) {
                Some((_, best)) => *best = (*best).min(rank),
                None => self.best_rank.push((source, rank)),
            }
        }
        fill(&mut self.dataset, &other.dataset);
        for key in other.keys {
            if !self.keys.contains(&key) {
                self.keys.push(key);
            }
        }
    }

    fn finish(self) -> Hit {
        let score = self.best_rank.iter().map(|(_, rank)| fusion(*rank)).sum();
        Hit {
            dataset: self.dataset,
            sources: self.best_rank.iter().map(|(s, _)| *s).collect(),
            score,
        }
    }
}

fn absorb(
    hits: &mut [Option<Building>],
    owner: &mut HashMap<String, usize>,
    into: usize,
    from: usize,
) {
    let Some(taken) = hits.get_mut(from).and_then(Option::take) else {
        return;
    };
    for key in &taken.keys {
        owner.insert(key.clone(), into);
    }
    if let Some(Some(target)) = hits.get_mut(into) {
        target.absorb(taken);
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "ranks are small; f64 holds them exactly"
)]
fn fusion(rank: usize) -> f64 {
    1.0 / (K + rank as f64 + 1.0)
}

/// Keep the first source's fields; take only what it lacked from later ones.
fn fill(target: &mut Dataset, other: &Dataset) {
    if target.description.is_none() {
        target.description.clone_from(&other.description);
    }
    if target.publisher.is_none() {
        target.publisher.clone_from(&other.publisher);
    }
    if target.doi.is_none() {
        target.doi.clone_from(&other.doi);
    }
    if target.license.is_none() {
        target.license.clone_from(&other.license);
    }
    if target.updated.is_none() {
        target.updated.clone_from(&other.updated);
    }
    if target.size_bytes.is_none() {
        target.size_bytes = other.size_bytes;
    }
}

/// Re-score fused hits by how much of the query they mention, because many
/// sources match loosely (any word, stemmed, or in fields dataseek never
/// sees). The fused score is scaled by `0.25 + 0.75 * coverage`, where
/// coverage is the share of query terms in the title or description. A hit
/// that shows a description yet mentions no term at all is dropped; one
/// without a description is kept at the floor, since there was little to
/// check it against.
pub fn weigh(hits: Vec<Hit>, query: &str) -> Vec<Hit> {
    let terms = crate::catalog::terms(query);
    if terms.is_empty() {
        return hits;
    }
    let needles = crate::catalog::needles(&terms);
    let kept: Vec<Hit> = hits
        .into_iter()
        .filter_map(|mut hit| {
            let text = format!(
                "{} {}",
                hit.dataset.title,
                hit.dataset.description.as_deref().unwrap_or("")
            );
            let found = crate::catalog::matched(&text, &needles);
            if found == 0 && hit.dataset.description.is_some() {
                return None;
            }
            hit.score *= 0.25 + 0.75 * share(found, terms.len());
            Some(hit)
        })
        .collect();
    rank(kept)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "term counts are tiny; f64 holds them exactly"
)]
fn share(found: usize, total: usize) -> f64 {
    found as f64 / total.max(1) as f64
}

/// Every key under which this record could appear in another source.
pub fn identity_keys(dataset: &Dataset) -> Vec<String> {
    let mut keys = Vec::new();
    let mut push = |key: String| {
        if !keys.contains(&key) {
            keys.push(key);
        }
    };
    if let Some(d) = dataset.doi.as_deref().and_then(doi) {
        push(format!("doi:{d}"));
    }
    for raw in std::iter::once(&dataset.url).chain(dataset.aliases.iter()) {
        if let Some(d) = doi_in_url(raw).or_else(|| bare_doi(raw)) {
            push(format!("doi:{d}"));
        } else if let Some(url) = normalize_url(raw) {
            push(format!("url:{url}"));
        }
    }
    if let Some(publisher) = &dataset.publisher {
        let title = fold(&dataset.title);
        if title.len() >= 12 {
            push(format!("title:{title}|{}", fold(publisher)));
        }
    }
    keys
}

/// A URL reduced to what identifies the page: no scheme, no `www.`, no
/// fragment, no trailing slash, no tracking parameters, lowercase host.
pub fn normalize_url(raw: &str) -> Option<String> {
    let rest = raw
        .trim()
        .strip_prefix("https://")
        .or_else(|| raw.trim().strip_prefix("http://"))?;
    let rest = rest.split('#').next().unwrap_or(rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("utm_"))
        .collect();
    let path = path.trim_end_matches('/');
    let mut out = format!("{host}/{path}");
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    (!host.is_empty()).then_some(out)
}

/// DOIs hide in resolver links and in `/doi/` paths (Zenodo, Wiley).
fn doi_in_url(raw: &str) -> Option<String> {
    if !raw.as_bytes().windows(3).any(|w| w.eq_ignore_ascii_case(b"doi")) {
        return None;
    }
    let lower = raw.to_lowercase();
    let after_resolver = ["doi.org/", "dx.doi.org/", "/doi/"]
        .iter()
        .find_map(|marker| lower.split_once(marker).map(|(_, tail)| tail))?;
    doi(after_resolver)
}

fn bare_doi(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (trimmed.starts_with("10.") || trimmed.starts_with("doi:"))
        .then(|| doi(trimmed))
        .flatten()
}

/// Lowercase alphanumerics separated by single spaces.
fn fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    let mut gap = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if gap && !folded.is_empty() {
                folded.push(' ');
            }
            gap = false;
            folded.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    folded
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    /// Records drawn from small pools of DOIs and links so they collide
    /// often; titles stay under the 12 characters a title key needs.
    fn colliding() -> impl Strategy<Value = Dataset> {
        (0..4_u8, prop::option::of(0..3_u8), prop::option::of(0..3_u8))
            .prop_map(|(page, doi, mirror)| {
                let mut d = Dataset::new(
                    &format!("Record {page}"),
                    &format!("https://x.org/{page}"),
                );
                d.doi = doi.map(|n| format!("10.1234/{n}"));
                d.aliases.extend(mirror.map(|n| format!("https://m.org/{n}")));
                d
            })
    }

    /// The groups `merge` should form, by brute force: records sharing any
    /// identity key, closed transitively, as the set of sources per group.
    fn expected_groups(
        lists: &[(&'static str, Vec<Dataset>)],
    ) -> Vec<BTreeSet<&'static str>> {
        let records: Vec<(&str, Vec<String>)> = lists
            .iter()
            .flat_map(|(s, ds)| ds.iter().map(|d| (*s, identity_keys(d))))
            .collect();
        let mut group: Vec<usize> = (0..records.len()).collect();
        let mut changed = true;
        while changed {
            changed = false;
            for i in 0..records.len() {
                for j in 0..records.len() {
                    let shared =
                        records[i].1.iter().any(|k| records[j].1.contains(k));
                    if shared && group[j] > group[i] {
                        group[j] = group[i];
                        changed = true;
                    }
                }
            }
        }
        let mut sets: Vec<BTreeSet<&str>> = BTreeSet::from_iter(group.clone())
            .into_iter()
            .map(|g| {
                records
                    .iter()
                    .zip(&group)
                    .filter(|(_, owner)| **owner == g)
                    .map(|((source, _), _)| *source)
                    .collect()
            })
            .collect();
        sets.sort();
        sets
    }

    proptest! {
        #[test]
        fn merging_groups_exactly_the_records_that_share_a_key(
            a in prop::collection::vec(colliding(), 0..5),
            b in prop::collection::vec(colliding(), 0..5),
            c in prop::collection::vec(colliding(), 0..5),
        ) {
            let lists = [("a", a), ("b", b), ("c", c)];
            let hits = merge(&lists);
            let mut groups: Vec<BTreeSet<&str>> = hits
                .iter()
                .map(|h| h.sources.iter().copied().collect())
                .collect();
            groups.sort();
            prop_assert_eq!(groups, expected_groups(&lists));
            prop_assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
        }
    }

    fn ds(title: &str, url: &str) -> Dataset {
        Dataset::new(title, url)
    }

    #[test]
    fn the_same_doi_from_two_sources_merges_into_one_hit() {
        let mut a = ds("Global temps", "https://zenodo.org/records/1");
        a.doi = Some("10.5281/zenodo.1".into());
        let b = ds("Global temperatures", "https://doi.org/10.5281/ZENODO.1");
        let hits = merge(&[("zenodo", vec![a]), ("datacite", vec![b])]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].sources, vec!["zenodo", "datacite"]);
    }

    #[test]
    fn concept_doi_aliases_fold_versions_of_one_record() {
        let mut v1 = ds("BirthClim", "https://doi.org/10.5281/zenodo.11");
        v1.aliases.push("10.5281/zenodo.10".into());
        let mut v2 = ds("BirthClim", "https://doi.org/10.5281/zenodo.12");
        v2.aliases.push("10.5281/zenodo.10".into());
        let hits = merge(&[("datacite", vec![v1, v2])]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].sources, vec!["datacite"]);
    }

    #[test]
    fn a_record_bridging_two_hits_merges_them_transitively() {
        let a = ds("A", "https://example.org/a");
        let mut b = ds("B", "https://example.org/b");
        b.doi = Some("10.1234/x".into());
        let mut bridge = ds("C", "https://example.org/a/");
        bridge.doi = Some("10.1234/x".into());
        let hits =
            merge(&[("s1", vec![a]), ("s2", vec![b]), ("s3", vec![bridge])]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].sources.len(), 3);
    }

    #[test]
    fn agreement_outranks_a_single_first_place() {
        let shared = ds("Shared", "https://example.org/shared");
        let solo = ds("Solo", "https://example.org/solo");
        let filler = ds("Filler", "https://example.org/filler");
        let hits = merge(&[
            ("s1", vec![solo, filler.clone(), shared.clone()]),
            ("s2", vec![filler, shared]),
        ]);
        assert_eq!(hits[0].dataset.title, "Filler");
        assert_eq!(hits.last().unwrap().dataset.title, "Solo");
    }

    #[test]
    fn a_shared_title_merges_across_sources_but_not_within_one() {
        let mut a = ds("Unemployment rate by sex", "https://dc.org/a");
        a.publisher = Some("Data Commons".into());
        let mut b = ds("Unemployment rate by sex", "https://dc.org/b");
        b.publisher = Some("Data Commons".into());
        let mut c = ds("Unemployment rate by sex", "https://mirror.org/c");
        c.publisher = Some("Data Commons".into());
        let hits = merge(&[("datacommons", vec![a, b]), ("mirror", vec![c])]);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].sources, vec!["datacommons", "mirror"]);
    }

    #[test]
    fn later_sources_only_fill_gaps() {
        let mut a = ds("T", "https://example.org/x");
        a.license = Some("CC0".into());
        let mut b = ds("T", "https://example.org/x");
        b.license = Some("MIT".into());
        b.size_bytes = Some(5);
        let hits = merge(&[("a", vec![a]), ("b", vec![b])]);
        assert_eq!(hits[0].dataset.license.as_deref(), Some("CC0"));
        assert_eq!(hits[0].dataset.size_bytes, Some(5));
    }

    #[test]
    fn weighing_drops_off_topic_hits_and_favors_full_coverage() {
        let mut off_topic = ds("Family Life Survey 2000", "https://x.org/1");
        off_topic.description = Some("Indonesia, 2000".into());
        let partial = ds("Lung function tests", "https://x.org/2");
        let full =
            ds("Single-cell atlas of the human lung", "https://x.org/3");
        let hits = merge(&[("a", vec![off_topic, partial, full])]);
        let titles: Vec<String> = weigh(hits, "single cell lung")
            .into_iter()
            .map(|h| h.dataset.title)
            .collect();
        assert_eq!(
            titles,
            ["Single-cell atlas of the human lung", "Lung function tests"]
        );
    }

    #[test]
    fn urls_normalize_scheme_host_slash_fragment_and_tracking() {
        assert_eq!(
            normalize_url("https://WWW.Kaggle.com/datasets/a/b/#__sid=js0"),
            normalize_url("http://kaggle.com/datasets/a/b?utm_source=x")
        );
        assert_ne!(
            normalize_url("https://x.org/d?id=1"),
            normalize_url("https://x.org/d?id=2")
        );
        assert_eq!(normalize_url("ftp://x.org"), None);
    }

    #[test]
    fn dois_inside_landing_links_become_doi_keys() {
        let d = ds("t", "https://zenodo.org/doi/10.5281/zenodo.99");
        assert!(identity_keys(&d).contains(&"doi:10.5281/zenodo.99".into()));
    }
}
