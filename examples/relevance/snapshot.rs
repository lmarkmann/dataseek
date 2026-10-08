//! The benchmark's files: the queries, the frozen per-source lists, the
//! judgments, and the labelling worksheets.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
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
    parse_queries(&read(&path)?, &path)
}

fn parse_queries(text: &str, path: &Path) -> Result<Vec<Query>> {
    let queries = toml::from_str::<QueryFile>(text)
        .with_context(|| format!("parsing {}", path.display()))?
        .query;
    let mut seen = HashSet::new();
    if let Some(twice) = queries.iter().find(|q| !seen.insert(&q.id)) {
        bail!("{}: query id {:?} appears twice", path.display(), twice.id);
    }
    Ok(queries)
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))
}

/// The file's text, or `None` when it does not exist; any other failure to
/// read it is an error.
pub fn read_if_present(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => {
            Err(e).with_context(|| format!("reading {}", path.display()))
        }
    }
}

/// One source's answer to one query when it was recorded. `status` is the
/// label `Status::label` gave it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Answered {
    pub id: String,
    pub status: String,
    pub results: usize,
}

impl Answered {
    /// Whether the source returned a list, as `Status::answered` decides.
    pub fn answered(&self) -> bool {
        self.status == "ok"
            || self.status == "cached"
            || self.status == "expired catalog"
            || self.status.starts_with("stale cache")
    }

    /// Whether the source was asked at all, as `Status::attempted` decides:
    /// not skipped for want of a key or after an outage.
    pub fn attempted(&self) -> bool {
        !self.status.starts_with("needs $")
            && !self.status.starts_with("skipped, outage")
    }
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
    /// A source that did not answer has an empty list.
    pub lists: Lists,
    /// How each source answered, in the same order.
    pub sources: Vec<Answered>,
    /// The release that recorded the snapshot; splicing one source in
    /// keeps it.
    pub dataseek: String,
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
    let text = read(&path).context("`just relevance record` writes it")?;
    parse_retrieval(&text, &path, query)
}

fn header(text: &str, path: &Path) -> Result<Header> {
    serde_json::from_str(text.lines().next().unwrap_or(""))
        .with_context(|| format!("the header of {}", path.display()))
}

fn parse_retrieval(
    text: &str,
    path: &Path,
    query: &Query,
) -> Result<Retrieval> {
    let header = header(text, path)?;
    if header.query != query.text {
        bail!(
            "{} was recorded for \"{}\", not \"{}\"; re-record it",
            path.display(),
            header.query,
            query.text
        );
    }
    let mut rows: HashMap<String, Vec<(usize, Dataset)>> = HashMap::new();
    for (i, line) in text.lines().enumerate().skip(1) {
        let at = || format!("{} line {}", path.display(), i.saturating_add(1));
        let row: Row = serde_json::from_str(line).with_context(at)?;
        if !header.sources.iter().any(|s| s.id == row.source) {
            bail!("{}: source {:?} is not in the header", at(), row.source);
        }
        rows.entry(row.source).or_default().push((row.rank, row.dataset));
    }
    let lists = header
        .sources
        .iter()
        .map(|s| {
            let mut ranked = rows.remove(&s.id).unwrap_or_default();
            if ranked.len() != s.results {
                bail!(
                    "{}: the header gives {} {} results, the file holds {}",
                    path.display(),
                    s.id,
                    s.results,
                    ranked.len()
                );
            }
            ranked.sort_by_key(|(rank, _)| *rank);
            Ok((
                static_id(&s.id),
                ranked.into_iter().map(|(_, d)| d).collect(),
            ))
        })
        .collect::<Result<_>>()?;
    Ok(Retrieval { lists, sources: header.sources, dataseek: header.dataseek })
}

/// How each source answered when `query` was last recorded; empty when it
/// never was.
pub fn recorded(query: &Query) -> Result<Vec<Answered>> {
    let path = retrieval_path(&query.id);
    let Some(text) = read_if_present(&path)? else {
        return Ok(Vec::new());
    };
    Ok(header(&text, &path)?.sources)
}

pub fn save(
    query: &Query,
    dataseek: &str,
    sources: &[Answered],
    lists: &Lists,
) -> Result<()> {
    let mut out = serde_json::to_string(&Header {
        query: query.text.clone(),
        dataseek: dataseek.to_owned(),
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
    pub fn keys(&self) -> Vec<String> {
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

/// Every label in judgments.tsv; none when the file does not exist yet.
pub fn labels() -> Result<Vec<Label>> {
    let path = judgments_path();
    match read_if_present(&path)? {
        Some(text) => parse_labels(&text, &path),
        None => Ok(Vec::new()),
    }
}

/// The labels in `text`. Two labels of one query that share an identity
/// key would grade one dataset twice, so they are an error.
fn parse_labels(text: &str, path: &Path) -> Result<Vec<Label>> {
    let mut labels = Vec::new();
    let mut first: HashMap<(String, String), usize> = HashMap::new();
    for (i, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let number = i.saturating_add(1);
        let at = format!("{} line {number}", path.display());
        let fields: Vec<&str> = line.split('\t').collect();
        let &[query, grade, url, doi, title, reason] = fields.as_slice()
        else {
            bail!("{at}: expected 6 tab-separated fields");
        };
        let label = Label {
            query: query.to_owned(),
            grade: parse_grade(grade).context(at.clone())?,
            url: url.to_owned(),
            doi: doi.to_owned(),
            title: title.to_owned(),
            reason: reason.to_owned(),
        };
        for key in label.keys() {
            if let Some(line) =
                first.insert((label.query.clone(), key.clone()), number)
            {
                bail!(
                    "{at}: {} labels {key} again, after line {line}; keep one",
                    label.query
                );
            }
        }
        labels.push(label);
    }
    Ok(labels)
}

pub fn parse_grade(text: &str) -> Result<Option<Grade>> {
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
        writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}",
            l.query,
            shown(l.grade),
            l.url,
            l.doi,
            tabless(&l.title),
            tabless(&l.reason)
        )?;
    }
    fs::write(judgments_path(), out)?;
    Ok(())
}

fn shown(grade: Option<Grade>) -> String {
    grade.map_or_else(|| "?".to_owned(), |g| g.to_string())
}

pub fn tabless(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// The labels of one query, found by any identity key a record carries,
/// with one grade per dataset.
#[derive(Clone)]
pub struct Judged {
    by_key: HashMap<String, usize>,
    pub grades: Vec<Option<Grade>>,
}

impl Judged {
    /// Labels sharing an identity key are one dataset and keep one grade;
    /// grading it differently twice is an error.
    pub fn new(labels: &[Label], query: &str) -> Result<Self> {
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let mut grades: Vec<Option<Grade>> = Vec::new();
        for label in labels.iter().filter(|l| l.query == query) {
            let keys = label.keys();
            let mut earlier: Vec<usize> =
                keys.iter().filter_map(|k| by_key.get(k).copied()).collect();
            earlier.sort_unstable();
            earlier.dedup();
            let index = match *earlier.as_slice() {
                [] => {
                    grades.push(label.grade);
                    grades.len().saturating_sub(1)
                }
                [index] => {
                    let was = grades.get(index).copied().flatten();
                    if was != label.grade {
                        bail!(
                            "{query}: {} is graded {} and {} under one identity; keep one",
                            label.url,
                            shown(was),
                            shown(label.grade)
                        );
                    }
                    index
                }
                _ => bail!(
                    "{query}: {} shares an identity with two other labels; keep one",
                    label.url
                ),
            };
            for key in keys {
                by_key.entry(key).or_insert(index);
            }
        }
        Ok(Self { by_key, grades })
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

/// A label of query "q" titled "t", for tests.
#[cfg(test)]
pub fn label(url: &str, doi: &str, grade: Grade) -> Label {
    Label {
        query: "q".into(),
        grade: Some(grade),
        url: url.into(),
        doi: doi.into(),
        title: "t".into(),
        reason: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dataseek::internals::Status;

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
        let judged = Judged::new(
            &[label("https://zenodo.org/records/1", "10.5281/zenodo.1", 2)],
            "q",
        )
        .unwrap();
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

    #[test]
    fn labels_sharing_an_identity_grade_one_dataset_once() {
        let twice = [
            label("https://zenodo.org/records/1", "10.5281/zenodo.1", 2),
            label("https://doi.org/10.5281/zenodo.1", "10.5281/zenodo.1", 2),
            label("https://x.org/other", "", 1),
        ];
        assert_eq!(Judged::new(&twice, "q").unwrap().all(), [2, 1]);
        let conflicting = [
            label("https://zenodo.org/records/1", "10.5281/zenodo.1", 2),
            label("https://doi.org/10.5281/ZENODO.1", "", 0),
        ];
        assert!(Judged::new(&conflicting, "q").is_err());
    }

    #[test]
    fn a_dataset_labelled_twice_in_one_query_is_refused() {
        let row = |query: &str, url: &str, doi: &str| {
            format!("{query}\t2\t{url}\t{doi}\tt\tr\n")
        };
        let path = Path::new("judgments.tsv");
        let apart = row("a", "https://x.org/1", "10.1234/x")
            + &row("b", "https://doi.org/10.1234/x", "");
        assert_eq!(parse_labels(&apart, path).unwrap().len(), 2);
        let twice = format!(
            "# rubric\n{}{}",
            row("a", "https://x.org/1", "10.1234/x"),
            row("a", "https://doi.org/10.1234/X", "")
        );
        let err = format!("{:#}", parse_labels(&twice, path).unwrap_err());
        assert!(err.contains("line 3") && err.contains("line 2"), "{err}");
    }

    #[test]
    fn the_committed_judgments_match_the_queries_one_label_per_dataset() {
        let queries = queries().unwrap();
        let labels = labels().unwrap();
        for l in &labels {
            assert!(queries.iter().any(|q| q.id == l.query), "{}", l.query);
        }
        for q in queries.iter().filter(|q| !q.graded()) {
            assert!(!q.targets.is_empty(), "{} has no targets", q.id);
        }
        for q in queries.iter().filter(|q| q.graded()) {
            Judged::new(&labels, &q.id).unwrap();
        }
    }

    #[test]
    fn a_query_id_used_twice_is_refused() {
        let toml = "[[query]]\nid = \"a\"\ntext = \"x\"\nkind = \"topical\"\nhalf = \"tune\"\n";
        let path = Path::new("queries.toml");
        assert_eq!(parse_queries(toml, path).unwrap().len(), 1);
        let err = parse_queries(&toml.repeat(2), path).unwrap_err();
        assert!(err.to_string().contains("\"a\" appears twice"), "{err}");
    }

    #[test]
    fn a_snapshot_must_agree_with_its_header() {
        let query = Query {
            id: "q".into(),
            text: "q".into(),
            kind: Kind::Topical,
            half: Half::Tune,
            targets: Vec::new(),
        };
        let path = Path::new("q.jsonl");
        let header = |results: usize| {
            format!(
                "{{\"query\":\"q\",\"dataseek\":\"0\",\"sources\":[{{\"id\":\"zenodo\",\"status\":\"ok\",\"results\":{results}}},{{\"id\":\"who\",\"status\":\"answered HTTP 500\",\"results\":0}}]}}\n"
            )
        };
        let row = |source: &str| {
            format!(
                "{{\"source\":\"{source}\",\"rank\":0,\"title\":\"t\",\"url\":\"https://x.org\"}}\n"
            )
        };
        let good =
            parse_retrieval(&(header(1) + &row("zenodo")), path, &query)
                .unwrap();
        assert_eq!(good.lists.len(), 2);
        assert!(!good.sources[1].answered() && good.sources[1].attempted());
        let stray = header(1) + &row("zenodo") + &row("osf");
        let err = parse_retrieval(&stray, path, &query).err().unwrap();
        assert!(err.to_string().contains("\"osf\" is not in the header"));
        let short = parse_retrieval(&header(2), path, &query).err().unwrap();
        assert!(short.to_string().contains("zenodo 2 results"), "{short}");
    }

    #[test]
    fn statuses_read_back_as_the_search_decided_them() {
        let read = |status: &Status| Answered {
            id: "s".into(),
            status: status.label(),
            results: 0,
        };
        let resting = Status::Resting(Duration::from_secs(5));
        let running = Status::Running(Duration::from_secs(5));
        for status in [
            Status::Fetched,
            Status::Cached,
            Status::Expired,
            Status::Downloading,
            resting,
            running,
        ] {
            let a = read(&status);
            assert_eq!(a.answered(), status.answered(), "{}", a.status);
            assert_eq!(a.attempted(), status.attempted(), "{}", a.status);
        }
    }
}
