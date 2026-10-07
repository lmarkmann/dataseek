//! The benchmark's files: the queries, the frozen per-source lists, the
//! judgments, and the labelling worksheets.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dataseek::internals::{Dataset, Hit, SOURCES, identity_keys};
use serde::{Deserialize, Serialize};

use crate::metrics::Grade;

pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relevance")
}

pub fn worksheets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("target/relevance-pool")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Known,
    Topical,
    Natural,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Half {
    Tune,
    Held,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Query {
    pub id: String,
    pub text: String,
    pub kind: Kind,
    pub half: Half,
    #[serde(default)]
    pub targets: Vec<String>,
}

impl Query {
    pub fn graded(&self) -> bool {
        self.kind != Kind::Known
    }
}

#[derive(Deserialize)]
struct QueryFile {
    query: Vec<Query>,
}

pub fn queries() -> Result<Vec<Query>> {
    let path = dir().join("queries.toml");
    let text = fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(toml::from_str::<QueryFile>(&text)?.query)
}

/// One source's answer to one query when it was recorded.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Answered {
    pub id: String,
    pub status: String,
    pub results: usize,
}

#[derive(Serialize, Deserialize)]
struct Header {
    query: String,
    dataseek: String,
    sources: Vec<Answered>,
}

#[derive(Serialize, Deserialize)]
struct Row {
    source: String,
    rank: usize,
    #[serde(flatten)]
    dataset: Dataset,
}

pub type Lists = Vec<(&'static str, Vec<Dataset>)>;

pub struct Retrieval {
    /// Each source's ranked list, in registry order, as `merge` takes them.
    pub lists: Lists,
}

fn retrieval_path(id: &str) -> PathBuf {
    dir().join("retrieval").join(format!("{id}.jsonl"))
}

/// The registry's own `&'static str` for an id, so recorded lists merge
/// exactly as live ones do. A source since removed from the registry still
/// loads, under a leaked copy of its id.
fn static_id(id: &str) -> &'static str {
    SOURCES
        .iter()
        .find(|s| s.id == id)
        .map_or_else(|| &*Box::leak(id.to_owned().into_boxed_str()), |s| s.id)
}

pub fn load(query: &Query) -> Result<Retrieval> {
    let path = retrieval_path(&query.id);
    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "reading {}; `just relevance record` writes it",
            path.display()
        )
    })?;
    let mut lines = text.lines();
    let header: Header = serde_json::from_str(lines.next().unwrap_or(""))
        .with_context(|| format!("the header of {}", path.display()))?;
    if header.query != query.text {
        bail!(
            "{} was recorded for \"{}\", not \"{}\"; re-record it",
            path.display(),
            header.query,
            query.text
        );
    }
    let mut rows: HashMap<String, Vec<(usize, Dataset)>> = HashMap::new();
    for line in lines {
        let row: Row = serde_json::from_str(line)
            .with_context(|| format!("a record in {}", path.display()))?;
        rows.entry(row.source).or_default().push((row.rank, row.dataset));
    }
    let lists = header
        .sources
        .iter()
        .map(|s| {
            let mut ranked = rows.remove(&s.id).unwrap_or_default();
            ranked.sort_by_key(|(rank, _)| *rank);
            (static_id(&s.id), ranked.into_iter().map(|(_, d)| d).collect())
        })
        .collect();
    Ok(Retrieval { lists })
}

pub fn save(query: &Query, sources: &[Answered], lists: &Lists) -> Result<()> {
    let mut out = serde_json::to_string(&Header {
        query: query.text.clone(),
        dataseek: env!("CARGO_PKG_VERSION").to_owned(),
        sources: sources.to_vec(),
    })?;
    out.push('\n');
    for (source, datasets) in lists {
        for (rank, dataset) in datasets.iter().enumerate() {
            let row = Row {
                source: (*source).to_owned(),
                rank,
                dataset: dataset.clone(),
            };
            out.push_str(&serde_json::to_string(&row)?);
            out.push('\n');
        }
    }
    let path = retrieval_path(&query.id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, out)
        .with_context(|| format!("writing {}", path.display()))
}

/// What the snapshot keeps of a record: what merging and ranking read, with
/// e-mail addresses replaced. License, size and date are dropped; no
/// ranking reads them.
pub fn frozen(mut dataset: Dataset) -> Dataset {
    dataset.license = None;
    dataset.size_bytes = None;
    dataset.updated = None;
    dataset.title = scrub(&dataset.title);
    dataset.description = dataset.description.as_deref().map(scrub);
    dataset.publisher = dataset.publisher.as_deref().map(scrub);
    dataset
}

const STANDIN: &str = "contact@example.org";

/// Every e-mail address in `text` replaced by [`STANDIN`]. An address is a
/// run of local-part characters, an `@`, and a domain holding a dot.
pub fn scrub(text: &str) -> String {
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let domain = |c: char| c.is_ascii_alphanumeric() || ".-".contains(c);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let (before, after) = rest.split_at(at);
        let after = after.get(1..).unwrap_or("");
        let start = before
            .char_indices()
            .rev()
            .find(|(_, c)| !local(*c))
            .map_or(0, |(i, c)| i.saturating_add(c.len_utf8()));
        let host_len = after.find(|c: char| !domain(c)).unwrap_or(after.len());
        let host = after.get(..host_len).unwrap_or("").trim_end_matches('.');
        let is_address = start < before.len()
            && host.contains('.')
            && !host.starts_with('.');
        if is_address {
            out.push_str(before.get(..start).unwrap_or(""));
            out.push_str(STANDIN);
            rest = after.get(host.len()..).unwrap_or("");
        } else {
            out.push_str(before);
            out.push('@');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// One judgment: how relevant one record is to one query, and why.
#[derive(Clone, Debug)]
pub struct Label {
    pub query: String,
    pub grade: Option<Grade>,
    pub url: String,
    pub doi: String,
    pub title: String,
    pub reason: String,
}

impl Label {
    fn keys(&self) -> Vec<String> {
        let mut record = Dataset::new(&self.title, &self.url);
        record.doi = (!self.doi.is_empty()).then(|| self.doi.clone());
        identity_keys(&record)
    }
}

const RUBRIC: &str = "\
# Relevance judgments for queries.toml, one record per line:
#   query <TAB> grade <TAB> url <TAB> doi <TAB> title <TAB> reason
#
# A label is matched to a merged result by identity (DOI or normalized URL),
# so it holds for that dataset whichever source returned it. Grades judge
# the record as dataseek shows it: title, description, publisher.
#
#   2  relevant: a dataset whose substance answers the query, with no
#      constraint the query states (period, region, frequency, modality)
#      contradicted
#   1  partly: on topic but missing or contradicting a stated constraint, a
#      broad collection that contains the answer, or a derived product
#   0  not relevant: off topic, a match on words only, or not data at all
#   ?  not yet judged; `just relevance` fails until none is left
#
# A record with too little text to tell gets the grade its title supports;
# a title that states exactly the need earns 2. Data finer than a stated
# frequency (hourly for daily) meets it, since it sums to it.
#
# The reason is one line drawn from the record itself. Edit a grade or a
# reason in place; `just relevance absorb` keeps edits and sorts the file.
";

pub fn judgments_path() -> PathBuf {
    dir().join("judgments.tsv")
}

pub fn labels() -> Result<Vec<Label>> {
    let path = judgments_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.starts_with('#') && !line.trim().is_empty())
        .map(|(i, line)| {
            let fields: Vec<&str> = line.split('\t').collect();
            let &[query, grade, url, doi, title, reason] = fields.as_slice()
            else {
                bail!(
                    "judgments.tsv line {}: expected 6 fields",
                    i.saturating_add(1)
                );
            };
            Ok(Label {
                query: query.to_owned(),
                grade: parse_grade(grade).with_context(|| {
                    format!("judgments.tsv line {}", i.saturating_add(1))
                })?,
                url: url.to_owned(),
                doi: doi.to_owned(),
                title: title.to_owned(),
                reason: reason.to_owned(),
            })
        })
        .collect()
}

fn parse_grade(text: &str) -> Result<Option<Grade>> {
    match text.trim() {
        "?" => Ok(None),
        "0" => Ok(Some(0)),
        "1" => Ok(Some(1)),
        "2" => Ok(Some(2)),
        other => bail!("grade {other:?} is not 0, 1, 2 or ?"),
    }
}

/// Labels sorted by query (in the order of `queries`), grade (best first),
/// then title.
pub fn write_labels(queries: &[Query], labels: &mut [Label]) -> Result<()> {
    let order: HashMap<&str, usize> =
        queries.iter().enumerate().map(|(i, q)| (q.id.as_str(), i)).collect();
    labels.sort_by(|a, b| {
        let at = |l: &Label| order.get(l.query.as_str()).copied();
        at(a)
            .cmp(&at(b))
            .then_with(|| b.grade.cmp(&a.grade))
            .then_with(|| a.title.cmp(&b.title))
    });
    let mut out = String::from(RUBRIC);
    for l in labels.iter() {
        let grade = l.grade.map_or_else(|| "?".to_owned(), |g| g.to_string());
        writeln!(
            out,
            "{}\t{grade}\t{}\t{}\t{}\t{}",
            l.query,
            l.url,
            l.doi,
            tabless(&l.title),
            tabless(&l.reason)
        )?;
    }
    fs::write(judgments_path(), out)?;
    Ok(())
}

pub fn tabless(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// The labels of one query, found by any identity key a record carries.
#[derive(Clone)]
pub struct Judged {
    by_key: HashMap<String, usize>,
    pub grades: Vec<Option<Grade>>,
}

impl Judged {
    pub fn new(labels: &[Label], query: &str) -> Self {
        let mut by_key = HashMap::new();
        let mut grades = Vec::new();
        for label in labels.iter().filter(|l| l.query == query) {
            for key in label.keys() {
                by_key.entry(key).or_insert(grades.len());
            }
            grades.push(label.grade);
        }
        Self { by_key, grades }
    }

    /// The grade of the label matching this record; `None` when no label
    /// matches or the one that does is still `?`.
    pub fn grade(&self, dataset: &Dataset) -> Option<Grade> {
        self.grades.get(self.index(dataset)?).copied().flatten()
    }

    /// Which label matches this record, if any.
    pub fn index(&self, dataset: &Dataset) -> Option<usize> {
        identity_keys(dataset).iter().find_map(|k| self.by_key.get(k)).copied()
    }

    /// A copy with one label's grade replaced.
    pub fn with(&self, index: usize, grade: Grade) -> Self {
        let mut copy = self.clone();
        if let Some(slot) = copy.grades.get_mut(index) {
            *slot = Some(grade);
        }
        copy
    }

    /// Every judged grade, for the ideal ordering.
    pub fn all(&self) -> Vec<Grade> {
        self.grades.iter().flatten().copied().collect()
    }
}

/// The identity keys of a known-item query's targets.
pub fn target_keys(query: &Query) -> Vec<String> {
    query
        .targets
        .iter()
        .flat_map(|t| {
            let mut record = Dataset::new("target", t);
            if !t.starts_with("http") {
                record.url = String::new();
                record.doi = Some(t.clone());
            }
            identity_keys(&record)
        })
        .collect()
}

pub fn is_target(hit: &Hit, targets: &[String]) -> bool {
    identity_keys(&hit.dataset).iter().any(|k| targets.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_replaces_addresses_and_leaves_other_at_signs() {
        assert_eq!(
            scrub("Contact: jane.doe@uni-x.edu. Measured @ 2 m."),
            "Contact: contact@example.org. Measured @ 2 m."
        );
        assert_eq!(scrub("(a+b@lab.org)"), "(contact@example.org)");
        assert_eq!(
            scrub("user@localhost and @handle"),
            "user@localhost and @handle"
        );
        assert_eq!(scrub("Zürich: x@y.ch"), "Zürich: contact@example.org");
    }

    #[test]
    fn a_label_matches_a_record_by_doi_or_normalized_url() {
        let label = |url: &str, doi: &str| Label {
            query: "q".into(),
            grade: Some(2),
            url: url.into(),
            doi: doi.into(),
            title: "t".into(),
            reason: String::new(),
        };
        let judged = Judged::new(
            &[label("https://zenodo.org/records/1", "10.5281/zenodo.1")],
            "q",
        );
        let by_url = Dataset::new("x", "http://www.zenodo.org/records/1/");
        let mut by_doi = Dataset::new("x", "https://elsewhere.org/a");
        by_doi.doi = Some("10.5281/ZENODO.1".into());
        assert_eq!(judged.grade(&by_url), Some(2));
        assert_eq!(judged.grade(&by_doi), Some(2));
        assert_eq!(
            judged.grade(&Dataset::new("x", "https://other.org")),
            None
        );
    }

    #[test]
    fn targets_accept_bare_dois_and_links() {
        let query = Query {
            id: "q".into(),
            text: "q".into(),
            kind: Kind::Known,
            half: Half::Tune,
            targets: vec![
                "10.1234/ABC".into(),
                "https://www.openml.org/d/61".into(),
            ],
        };
        let keys = target_keys(&query);
        assert!(keys.contains(&"doi:10.1234/abc".to_owned()), "{keys:?}");
        assert!(keys.contains(&"url:openml.org/d/61".to_owned()), "{keys:?}");
    }
}
