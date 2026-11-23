//! `related`: the datasets beside one item, read from the item's own host
//! (ADR 0021). The URL's host and path choose one resolver: a Hugging Face
//! dataset or owner page lists the owner's datasets, a paper page or an
//! arXiv link the datasets that cite the paper, a model page the datasets
//! its card names, a collection page its datasets; a GitHub page lists the
//! owner's repositories or a repository's forks; a GitLab project page lists
//! the projects of its group or user. One host is asked, once (GitLab twice:
//! the project first says whose it is), and the order is the host's. Nothing
//! fans out, merges or is cached.
//!
//! Each row maps through the function its source's adapter uses, so one JSON
//! row is one record in `search` and here. A model's tags carry dataset ids
//! alone, so those records hold the id and the link.
//!
//! The API is asked on the scheme the URL names, and a key travels only over
//! `https`.

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use crate::cli::By;
use crate::credentials::{Credentials, Key};
use crate::find;
use crate::http::{Call, Http, SourceError};
use crate::output::Out;
use crate::record::{Dataset, text};
use crate::sources::{HUB_FIELDS, github_record, gitlab_record, hub_record};
use crate::ui;

/// The most datasets one Hub listing returns.
const HUB_PAGE: usize = 1000;
/// The most repositories one GitHub or GitLab page holds.
const PAGE: usize = 100;
/// First path segments of huggingface.co that are not an owner.
const HUB_PAGES: [&str; 8] = [
    "datasets",
    "models",
    "spaces",
    "papers",
    "collections",
    "docs",
    "blog",
    "api",
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "{url} is not a page `related` reads\n  Try:   a Hugging Face dataset, owner, model, paper or collection page, an arXiv link, a GitHub repository or owner page, or a GitLab project page"
    )]
    Unsupported { url: String },
    #[error(
        "only a GitHub repository has forks to list\n  Try:   drop --by forks, or give a page under github.com/OWNER/REPO"
    )]
    NoForks,
    #[error(
        "cannot reach {url}\n  Try:   check the address and the connection, then run it again"
    )]
    Unreachable {
        url: String,
        #[source]
        source: SourceError,
    },
    #[error("cannot verify the certificate of {url}\n  Try:   {hint}")]
    Certificate {
        url: String,
        /// From [`crate::http::certificate_hint`].
        hint: String,
        #[source]
        source: SourceError,
    },
    #[error(
        "related asks the item's host, so it needs the network\n  Try:   run it again without --offline"
    )]
    Offline,
    #[error(
        "cannot read what is beside {url}\n  Try:   open the page in a browser to check that it exists, then run it again"
    )]
    Fetch {
        url: String,
        #[source]
        source: SourceError,
    },
}

/// What one URL asks of which host.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// `item` is the dataset the URL named, absent for an owner page.
    HubOwner {
        owner: String,
        item: Option<String>,
    },
    HubPaper {
        arxiv: String,
    },
    HubModel {
        id: String,
    },
    HubCollection {
        slug: String,
    },
    /// `item` is the repository the URL named, absent for an owner page.
    GitHubOwner {
        owner: String,
        item: Option<String>,
    },
    GitHubForks {
        repo: String,
    },
    GitLabOwner {
        project: String,
    },
}

impl Plan {
    fn source(&self) -> &'static str {
        match self {
            Self::HubOwner { .. }
            | Self::HubPaper { .. }
            | Self::HubModel { .. }
            | Self::HubCollection { .. } => "huggingface",
            Self::GitHubOwner { .. } | Self::GitHubForks { .. } => "github",
            Self::GitLabOwner { .. } => "gitlab",
        }
    }

    fn relation(&self) -> &'static str {
        match self {
            Self::HubOwner { .. }
            | Self::GitHubOwner { .. }
            | Self::GitLabOwner { .. } => "owner",
            Self::HubPaper { .. } => "paper",
            Self::HubModel { .. } => "model",
            Self::HubCollection { .. } => "collection",
            Self::GitHubForks { .. } => "forks",
        }
    }

    /// The item the URL named, which is not its own neighbor.
    fn item(&self) -> Option<&str> {
        match self {
            Self::HubOwner { item, .. } | Self::GitHubOwner { item, .. } => {
                item.as_deref()
            }
            Self::GitLabOwner { project } => Some(project),
            _ => None,
        }
    }
}

pub fn run(url: &str, by: By, limit: usize, out: &Out) -> Result<()> {
    let (plan, secure) = plan(url, by)?;
    let dirs = crate::paths::resolve()?;
    let asked = Asked {
        http: &Http::new(),
        creds: &Credentials::load(&dirs.config),
        secure,
    };
    let progress = ui::spinner(format!("reading what is beside {url}"));
    let found =
        fetch(&plan, &asked, limit).and_then(|body| datasets(&plan, &body));
    progress.finish_and_clear();
    let mut found = found.map_err(|e| match e {
        SourceError::Offline => Error::Offline,
        SourceError::Unreachable(_) => {
            Error::Unreachable { url: url.to_owned(), source: e }
        }
        SourceError::Certificate(_) => Error::Certificate {
            url: url.to_owned(),
            hint: crate::http::certificate_hint(),
            source: e,
        },
        _ => Error::Fetch { url: url.to_owned(), source: e },
    })?;
    let total = found.len();
    found.truncate(limit);
    print(out, url, &plan, &found)?;
    match (found.len(), total) {
        (_, 0) => ui::warn(format!("no datasets beside {url}")),
        (shown, total) if shown < total => ui::ok(format!(
            "{shown} of {}; -n {total} shows all",
            ui::count(total, "dataset")
        )),
        _ => ui::ok(ui::count(total, "dataset")),
    }
    Ok(())
}

/// The resolver for `url` and whether the URL is `https`.
fn plan(url: &str, by: By) -> Result<(Plan, bool), Error> {
    let unknown = || Error::Unsupported { url: url.to_owned() };
    let (scheme, rest) = url.trim().split_once("://").ok_or_else(unknown)?;
    let secure = match scheme {
        "https" => true,
        "http" => false,
        _ => return Err(unknown()),
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let host = parts.next().ok_or_else(unknown)?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    let path: Vec<&str> = parts.collect();
    // A GitLab project path ends where `/-/` starts a page inside it; on the
    // other hosts nothing past the third segment names the item.
    let named = if host == "gitlab.com" {
        path.iter().position(|part| *part == "-").unwrap_or(path.len())
    } else {
        path.len().min(3)
    };
    let path = path.get(..named).unwrap_or_default();
    if !path.iter().all(|part| plain(part)) {
        return Err(unknown());
    }
    let plan = match (host, path) {
        ("huggingface.co", ["datasets", owner, name]) => Plan::HubOwner {
            owner: (*owner).to_owned(),
            item: Some(format!("{owner}/{name}")),
        },
        ("huggingface.co", ["papers", id, ..])
        | ("arxiv.org", ["abs" | "pdf", id, ..]) => {
            Plan::HubPaper { arxiv: arxiv_id(id).ok_or_else(unknown)? }
        }
        ("huggingface.co", ["collections", owner, slug]) => {
            Plan::HubCollection { slug: format!("{owner}/{slug}") }
        }
        ("huggingface.co", [first, ..]) if HUB_PAGES.contains(first) => {
            return Err(unknown());
        }
        ("huggingface.co", [owner]) => {
            Plan::HubOwner { owner: (*owner).to_owned(), item: None }
        }
        ("huggingface.co", [owner, model, ..]) => {
            Plan::HubModel { id: format!("{owner}/{model}") }
        }
        ("github.com", [owner, repo, ..]) if by == By::Forks => {
            Plan::GitHubForks { repo: format!("{owner}/{repo}") }
        }
        ("github.com", [owner, rest @ ..]) => Plan::GitHubOwner {
            owner: (*owner).to_owned(),
            item: rest.first().map(|repo| format!("{owner}/{repo}")),
        },
        ("gitlab.com", [_, _, ..]) => {
            Plan::GitLabOwner { project: path.join("/") }
        }
        _ => return Err(unknown()),
    };
    if by == By::Forks && !matches!(plan, Plan::GitHubForks { .. }) {
        return Err(Error::NoForks);
    }
    Ok((plan, secure))
}

/// Whether a path segment is a name the hosts allow: nothing in it can
/// change the request it becomes part of.
fn plain(part: &str) -> bool {
    part.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// An arXiv id of the form in use since 2007, without its version or a
/// `.pdf` ending: `1810.04805v2` is `1810.04805`.
fn arxiv_id(raw: &str) -> Option<String> {
    let raw = raw.strip_suffix(".pdf").unwrap_or(raw);
    let id = match raw.rsplit_once('v') {
        Some((id, version))
            if !version.is_empty()
                && version.chars().all(|c| c.is_ascii_digit()) =>
        {
            id
        }
        _ => raw,
    };
    let (month, number) = id.split_once('.')?;
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    (month.len() == 4 && digits(month) && !number.is_empty() && digits(number))
        .then(|| id.to_owned())
}

/// What a request needs beside its address.
struct Asked<'a> {
    http: &'a Http,
    creds: &'a Credentials,
    secure: bool,
}

impl Asked<'_> {
    fn call(&self, host: &str, path: &str, key: Option<Key>) -> Call<'_> {
        let scheme = if self.secure { "https" } else { "http" };
        let call = self.http.get(&format!("{scheme}://{host}/{path}"));
        match key.and_then(|key| self.creds.get(key)) {
            Some(secret) if self.secure => {
                call.key_header("Authorization", secret.authorization())
            }
            _ => call,
        }
    }

    fn hub(&self, path: &str) -> Call<'_> {
        self.call(
            "huggingface.co",
            &format!("api/{path}"),
            Some(Key::HuggingFace),
        )
    }

    fn github(&self, path: &str) -> Call<'_> {
        self.call("api.github.com", path, Some(Key::GitHub))
            .header("X-GitHub-Api-Version", "2022-11-28")
            .query("per_page", PAGE)
    }

    fn gitlab(&self, path: &str) -> Call<'_> {
        self.call("gitlab.com", &format!("api/v4/{path}"), None)
    }
}

/// The host's answer that holds the neighbors.
fn fetch(
    plan: &Plan,
    asked: &Asked<'_>,
    limit: usize,
) -> Result<Value, SourceError> {
    match plan {
        Plan::HubOwner { owner, .. } => {
            // One more than `limit`: the listing holds the named item too.
            let mut call = asked
                .hub("datasets")
                .query("author", owner)
                .query("sort", "downloads")
                .query("limit", limit.saturating_add(1).min(HUB_PAGE));
            for field in HUB_FIELDS {
                call = call.query("expand[]", field);
            }
            call.json()
        }
        Plan::HubPaper { arxiv } => {
            asked.hub(&format!("arxiv/{arxiv}/repos")).json()
        }
        Plan::HubModel { id } => asked.hub(&format!("models/{id}")).json(),
        Plan::HubCollection { slug } => {
            asked.hub(&format!("collections/{slug}")).json()
        }
        Plan::GitHubOwner { owner, .. } => asked
            .github(&format!("users/{owner}/repos"))
            .query("sort", "pushed")
            .json(),
        Plan::GitHubForks { repo } => asked
            .github(&format!("repos/{repo}/forks"))
            .query("sort", "stargazers")
            .json(),
        Plan::GitLabOwner { project } => {
            let project = asked
                .gitlab(&format!("projects/{}", escaped(project)))
                .json()?;
            let (kind, path) = namespace(&project)?;
            let mut call = asked
                .gitlab(&format!("{kind}/{}/projects", escaped(&path)))
                .query("order_by", "star_count")
                .query("sort", "desc")
                .query("per_page", PAGE)
                .query("simple", "true");
            if kind == "groups" {
                call = call.query("include_subgroups", "true");
            }
            call.json()
        }
    }
}

/// A GitLab path as one URL segment.
fn escaped(path: &str) -> String {
    path.replace('/', "%2F")
}

/// The API collection and the path of the namespace a GitLab project is in:
/// `groups` or `users`.
fn namespace(project: &Value) -> Result<(&'static str, String), SourceError> {
    let kind = match text(project, "/namespace/kind").as_deref() {
        Some("group") => "groups",
        Some("user") => "users",
        _ => return Err(SourceError::shape("no namespace kind")),
    };
    let path = text(project, "/namespace/full_path")
        .filter(|path| path.split('/').all(plain))
        .ok_or_else(|| SourceError::shape("no namespace path"))?;
    Ok((kind, path))
}

/// The neighbors in `body`, in the host's order, without the item the URL
/// named.
fn datasets(plan: &Plan, body: &Value) -> Result<Vec<Dataset>, SourceError> {
    let rows = |pointer: &str| {
        body.pointer(pointer)
            .and_then(Value::as_array)
            .ok_or_else(|| SourceError::shape("no list of neighbors"))
    };
    let found: Vec<Dataset> = match plan {
        Plan::HubOwner { .. } => {
            rows("")?.iter().filter_map(hub_record).collect()
        }
        Plan::HubPaper { .. } => {
            rows("/datasets")?.iter().filter_map(hub_record).collect()
        }
        Plan::HubModel { .. } => rows("/tags")?
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|tag| tag.strip_prefix("dataset:"))
            .filter_map(named)
            .collect(),
        Plan::HubCollection { .. } => rows("/items")?
            .iter()
            .filter(|item| text(item, "/type").as_deref() == Some("dataset"))
            .filter_map(hub_record)
            .collect(),
        Plan::GitHubOwner { .. } | Plan::GitHubForks { .. } => {
            rows("")?.iter().filter_map(github_record).collect()
        }
        Plan::GitLabOwner { .. } => {
            rows("")?.iter().filter_map(gitlab_record).collect()
        }
    };
    let item = plan.item();
    Ok(found
        .into_iter()
        .filter(|d| !item.is_some_and(|i| d.title.eq_ignore_ascii_case(i)))
        .collect())
}

/// The record of a Hub dataset known by its id alone.
fn named(id: &str) -> Option<Dataset> {
    Dataset::new(id, &format!("https://huggingface.co/datasets/{id}")).valid()
}

/// Tag of the `--json` shape; bumped when a key changes meaning or goes.
const SCHEMA: &str = "dataseek-related/1";

#[derive(Serialize)]
struct Report<'a> {
    schema: &'static str,
    url: &'a str,
    /// The registry id of the host that answered.
    source: &'static str,
    /// `owner`, `paper`, `model`, `collection` or `forks`.
    relation: &'static str,
    results: &'a [Dataset],
}

fn print(out: &Out, url: &str, plan: &Plan, found: &[Dataset]) -> Result<()> {
    if out.json {
        return out.json(&Report {
            schema: SCHEMA,
            url,
            source: plan.source(),
            relation: plan.relation(),
            results: found,
        });
    }
    let sources = [plan.source()];
    let shown: Vec<_> = found.iter().map(|d| (d, &sources[..])).collect();
    find::list(out, &shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::fixture;

    fn planned(url: &str, by: By) -> Plan {
        plan(url, by).unwrap().0
    }

    fn titles(found: &[Dataset]) -> Vec<&str> {
        found.iter().map(|d| d.title.as_str()).collect()
    }

    fn body(name: &str) -> Value {
        serde_json::from_str(&fixture::read("related", name)).unwrap()
    }

    #[test]
    fn a_url_names_its_resolver() {
        let hub = "https://huggingface.co";
        assert_eq!(
            planned(
                &format!("{hub}/datasets/nyu-mll/glue/tree/main"),
                By::Owner
            ),
            Plan::HubOwner {
                owner: "nyu-mll".into(),
                item: Some("nyu-mll/glue".into()),
            }
        );
        assert_eq!(
            planned(&format!("{hub}/nyu-mll"), By::Owner),
            Plan::HubOwner { owner: "nyu-mll".into(), item: None }
        );
        for paper in [
            format!("{hub}/papers/1810.04805"),
            "https://arxiv.org/abs/1810.04805v2".to_owned(),
            "https://arxiv.org/pdf/1810.04805.pdf".to_owned(),
        ] {
            assert_eq!(
                planned(&paper, By::Owner),
                Plan::HubPaper { arxiv: "1810.04805".into() }
            );
        }
        assert_eq!(
            planned(
                &format!("{hub}/google-bert/bert-base-uncased"),
                By::Owner
            ),
            Plan::HubModel { id: "google-bert/bert-base-uncased".into() }
        );
        assert_eq!(
            planned(
                &format!("{hub}/collections/AnPrudentia/datasets-6a9dd086"),
                By::Owner
            ),
            Plan::HubCollection {
                slug: "AnPrudentia/datasets-6a9dd086".into()
            }
        );
        let repo = "https://github.com/zalandoresearch/fashion-mnist";
        assert_eq!(
            planned(&format!("{repo}/issues?q=x"), By::Owner),
            Plan::GitHubOwner {
                owner: "zalandoresearch".into(),
                item: Some("zalandoresearch/fashion-mnist".into()),
            }
        );
        assert_eq!(
            planned(repo, By::Forks),
            Plan::GitHubForks { repo: "zalandoresearch/fashion-mnist".into() }
        );
        assert_eq!(
            planned(
                "https://gitlab.com/diw-evu/oet/openenergytracker/-/tree/main",
                By::Owner
            ),
            Plan::GitLabOwner {
                project: "diw-evu/oet/openenergytracker".into()
            }
        );
    }

    #[test]
    fn pages_without_neighbors_are_refused() {
        for url in [
            "https://zenodo.org/records/13135140",
            "https://huggingface.co/spaces/owner/demo",
            "https://huggingface.co/datasets/glue",
            "https://arxiv.org/abs/hep-th/9901001",
            "https://gitlab.com/restcountries",
            "https://github.com/",
            "ftp://github.com/owner/repo",
            "github.com/owner/repo",
            "https://github.com/owner/re%2Fpo",
        ] {
            assert!(
                matches!(plan(url, By::Owner), Err(Error::Unsupported { .. })),
                "{url}"
            );
        }
    }

    #[test]
    fn forks_exist_only_for_a_github_repository() {
        for url in [
            "https://huggingface.co/datasets/nyu-mll/glue",
            "https://github.com/zalandoresearch",
            "https://gitlab.com/restcountries/restcountries",
        ] {
            assert!(
                matches!(plan(url, By::Forks), Err(Error::NoForks)),
                "{url}"
            );
        }
    }

    #[test]
    fn a_key_is_sent_only_over_https() {
        let (_, secure) =
            plan("http://huggingface.co/nyu-mll", By::Owner).unwrap();
        assert!(!secure);
    }

    #[test]
    fn an_owners_datasets_leave_out_the_named_one() {
        let plan =
            planned("https://huggingface.co/datasets/nyu-mll/glue", By::Owner);
        let found = datasets(&plan, &body("hub-owner.json")).unwrap();
        assert_eq!(
            titles(&found),
            [
                "nyu-mll/blimp",
                "nyu-mll/multi_nli",
                "nyu-mll/crows_pairs",
                "nyu-mll/multi_nli_mismatch",
            ]
        );
        assert_eq!(
            found[2],
            Dataset {
                title: "nyu-mll/crows_pairs".into(),
                url: "https://huggingface.co/datasets/nyu-mll/crows_pairs"
                    .into(),
                description: Some(
                    "CrowS-Pairs, a challenge dataset for measuring the \
                     degree to which U.S. stereotypical biases present in \
                     the masked language models (MLMs)."
                        .into()
                ),
                publisher: Some("nyu-mll".into()),
                doi: None,
                license: Some("CC-BY-SA-4.0".into()),
                updated: Some("2024-01-18".into()),
                size_bytes: Some(10_100),
                popularity: Some(1026),
                formats: vec![],
                aliases: vec![],
            }
        );
    }

    #[test]
    fn a_papers_datasets_come_without_its_models_and_spaces() {
        let plan = planned("https://arxiv.org/abs/1810.04805", By::Owner);
        let found = datasets(&plan, &body("hub-paper.json")).unwrap();
        assert_eq!(
            titles(&found),
            [
                "Babelscape/wikineural",
                "avduarte333/arXivTection",
                "IberaSoft/ecommerce-reviews-sentiment",
            ]
        );
        assert_eq!(found[0].license.as_deref(), Some("CC-BY-NC-SA-4.0"));
        assert_eq!(found[0].updated.as_deref(), Some("2022-11-13"));
        assert_eq!(found[0].popularity, Some(5525));
    }

    #[test]
    fn a_models_dataset_tags_become_records() {
        let plan = planned(
            "https://huggingface.co/distilbert/distilbert-base-uncased-finetuned-sst-2-english",
            By::Owner,
        );
        let found = datasets(&plan, &body("hub-model.json")).unwrap();
        assert_eq!(titles(&found), ["sst2", "glue"]);
        assert_eq!(found[0].url, "https://huggingface.co/datasets/sst2");
        assert_eq!(found[0].publisher, None);
    }

    #[test]
    fn a_collections_datasets_come_without_its_models() {
        let plan = planned(
            "https://huggingface.co/collections/AnPrudentia/datasets-6a9dd08674b2a9ed59176852",
            By::Owner,
        );
        let found = datasets(&plan, &body("hub-collection.json")).unwrap();
        assert_eq!(
            titles(&found),
            [
                "spatialverse/UniPhys-Bench",
                "openbmb/Ultra-FineWeb-L1",
                "markov-ai/gaming-500-hours",
            ]
        );
        assert_eq!(found[0].updated.as_deref(), Some("2026-09-04"));
        assert_eq!(found[0].popularity, Some(14_856));
    }

    #[test]
    fn a_github_owners_repositories_map_as_search_results() {
        let plan = planned("https://github.com/zalandoresearch", By::Owner);
        let found = datasets(&plan, &body("github-owner.json")).unwrap();
        assert_eq!(
            titles(&found),
            [
                "zalandoresearch/batching-benchmarks",
                "zalandoresearch/pytorch-ts",
                "zalandoresearch/datacardsplaybook",
            ]
        );
        assert_eq!(found[0].license.as_deref(), Some("MIT"));
        assert_eq!(found[0].updated.as_deref(), Some("2026-09-16"));
        assert_eq!(found[0].size_bytes, Some(86_292_480));
        assert_eq!(found[0].popularity, Some(13));
    }

    #[test]
    fn the_forks_of_a_repository_are_listed() {
        let plan = planned(
            "https://github.com/zalandoresearch/fashion-mnist",
            By::Forks,
        );
        let found = datasets(&plan, &body("github-forks.json")).unwrap();
        assert_eq!(
            titles(&found),
            [
                "HaidaQuant/fashion-mnist",
                "JuanPuyo1/fashion-mnist",
                "EdsonMontanhini/fashion-mnist",
            ]
        );
        assert_eq!(found[0].publisher.as_deref(), Some("HaidaQuant"));
    }

    #[test]
    fn a_gitlab_project_names_the_group_or_user_it_is_in() {
        let project = body("gitlab-project.json");
        assert_eq!(
            namespace(&project).unwrap(),
            ("groups", "restcountries".to_owned())
        );
        let plan = planned(
            "https://gitlab.com/restcountries/restcountries",
            By::Owner,
        );
        let found = datasets(&plan, &fixture::json("gitlab.json")).unwrap();
        assert!(!titles(&found).contains(&"restcountries/restcountries"));
        assert_eq!(found.len(), 5);
    }

    #[test]
    fn an_answer_of_another_shape_is_an_error() {
        let plan = planned("https://huggingface.co/nyu-mll", By::Owner);
        let outcome = datasets(&plan, &serde_json::json!({"error": "x"}));
        assert!(matches!(outcome, Err(SourceError::Shape(_))));
    }
}
