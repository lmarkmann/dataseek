//! `search`: run the search loop, merge, and print. stdout carries the
//! merged results (text, `--plain` TSV, or `--json` with a per-source
//! report); stderr announces the search and, after the results, names the
//! sources that failed and sums up, so the last line says how it went.
//!
//! Terminal states: results found exits 0; nothing matched exits 0 with an
//! empty result set and a stderr line saying so; some sources failing is a
//! warning, not a failure; every attempted source failing exits 1, because
//! that is an offline machine or a broken network rather than an empty
//! answer (ADR 0007).

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::builder::styling::Style;
use serde::Serialize;

use crate::cache::Cache;
use crate::cli::{Selection, Sort};
use crate::dedup::{Hit, merge, weigh};
use crate::http::SourceError;
use crate::output::{self, Out, fit};
use crate::search::{Outcome, Plan, Status, run};
use crate::sources::{Services, select};
use crate::{palette, ui};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "every source failed, so there is nothing to show\n  Try:   check the connection, then `dataseek search ... -v` for each source's error"
    )]
    AllFailed,
    #[error(
        "no source could be reached; this machine looks offline\n  Try:   check the connection, or add --offline to search what is cached"
    )]
    Offline,
    /// The hint from [`crate::http::certificate_hint`].
    #[error(
        "no source's certificate could be verified, so none answered\n  Try:   {0}"
    )]
    Certificate(String),
    #[error(
        "no source answered before the deadline\n  Try:   search again in a minute, or add --timeout 0 to wait for every source"
    )]
    Unanswered,
    #[error(
        "nothing cached answers this query\n  Try:   run it once without --offline, or `dataseek cache warm` while online"
    )]
    NothingCached,
    #[error(
        "none of the chosen sources can run\n  Try:   `dataseek sources` shows which keys are missing"
    )]
    NothingRan,
    #[error(
        "every chosen source failed minutes ago and is resting, so none was asked\n  Try:   name them with -s to ask anyway, or add --offline to search what is cached"
    )]
    Resting,
    #[error(
        "no source is left after -s, -c and -x\n  Try:   `dataseek sources` lists the ids and categories; DATASEEK_EXCLUDE counts as -x"
    )]
    NoneSelected,
}

pub struct Request<'a> {
    pub words: &'a [String],
    pub selection: &'a Selection,
    pub limit: usize,
    pub sort: Sort,
    pub refresh: bool,
    pub offline: bool,
    /// Seconds to wait for the slowest sources; `None` waits for all.
    pub timeout: Option<u64>,
}

pub fn run_search(request: &Request<'_>, out: &Out) -> Result<()> {
    let Request { words, selection, limit, sort, refresh, offline, timeout } =
        *request;
    let services = Arc::new(Services::load()?);
    let query = words.join(" ");
    let plan = Arc::new(Plan {
        query: query.clone(),
        sources: select(
            &selection.only,
            &selection.exclude,
            &selection.categories,
        ),
        per_source: usize::from(selection.per_source),
        forced: offline || !selection.only.is_empty(),
    });
    if plan.sources.is_empty() {
        return Err(Error::NoneSelected.into());
    }

    // A source without its required key is skipped before any request, so
    // the line counts only the sources that will be asked.
    let keyless = plan
        .sources
        .iter()
        .filter(|s| s.missing_key(&services.creds).is_some())
        .count();
    let asked = plan.sources.len().saturating_sub(keyless);
    if asked > 0 {
        let need = if keyless == 1 { "needs" } else { "need" };
        let keys = if keyless > 0 {
            format!(
                "; {keyless} {need} a key, see `{} doctor`",
                crate::invoked_name()
            )
        } else {
            String::new()
        };
        ui::stage(format!(
            "searching {} for \"{query}\"{}{keys}",
            ui::count(asked, "source"),
            if offline { ", offline" } else { "" }
        ));
    }
    let progress = ui::bar(asked as u64, "sources");
    let mut outcomes = run(
        &services,
        refresh,
        &plan,
        &progress,
        timeout.map(Duration::from_secs),
    );
    progress.finish_and_clear();
    let renewing = renew(&services.cache, &outcomes);
    services.cache.trim();

    notes(out, &outcomes);
    if !outcomes.iter().any(|o| o.status.attempted()) {
        let resting =
            outcomes.iter().any(|o| matches!(o.status, Status::Resting(_)));
        return Err(
            if resting { Error::Resting } else { Error::NothingRan }.into()
        );
    }
    if !outcomes.iter().any(|o| o.status.answered()) {
        warn_failures(&outcomes, renewing, &services.cache);
        return Err(failure(&outcomes, offline).into());
    }

    let sources = reports(&outcomes);
    let mut hits = ranked(&mut outcomes, &query, sort);
    let found = hits.len();
    hits.truncate(limit);
    print(out, &query, &hits, sources)?;
    summarize(&query, hits.len(), found, &outcomes, renewing, &services.cache);
    Ok(())
}

/// Download again, in the background, the catalogs this search searched
/// expired or stopped waiting for. Whether that download is under way.
fn renew(cache: &Cache, outcomes: &[Outcome]) -> bool {
    let ids = to_renew(outcomes);
    !ids.is_empty()
        && !crate::http::is_offline()
        && crate::cache_cmd::warm_in_background(cache, &ids)
}

fn to_renew(outcomes: &[Outcome]) -> Vec<&'static str> {
    outcomes
        .iter()
        .filter(|o| o.source.is_catalog())
        .filter(|o| matches!(o.status, Status::Downloading | Status::Expired))
        .map(|o| o.source.id)
        .collect()
}

/// Catalogs searched from an expired copy, or not downloaded yet, whose
/// last download failed: the background download's failures surface here.
fn warn_download_failures(cache: &Cache, outcomes: &[Outcome]) {
    for o in outcomes.iter().filter(|o| o.source.is_catalog()) {
        let Some(reason) = cache.last_failure(o.source.id) else { continue };
        let id = o.source.id;
        match o.status {
            Status::Expired => ui::warn(format!(
                "{id} was searched from an expired catalog; its last download failed ({reason})"
            )),
            Status::Downloading => ui::warn(format!(
                "{id} has no catalog yet; its last download failed ({reason})"
            )),
            _ => {}
        }
    }
}

/// Every source's results, moved out of their outcomes, merged and ordered.
fn ranked(outcomes: &mut [Outcome], query: &str, sort: Sort) -> Vec<Hit> {
    let lists = outcomes
        .iter_mut()
        .map(|o| (o.source.id, std::mem::take(&mut o.datasets)))
        .collect();
    let mut hits = weigh(merge(lists), query);
    if sort == Sort::Newest {
        // Stable, so equally dated hits keep their relevance order; `None`
        // sorts below every date.
        hits.sort_by(|a, b| b.dataset.updated.cmp(&a.dataset.updated));
    }
    hits
}

/// Which failure every source failing amounts to: all unreachable reads as
/// an offline machine, all refusing their certificates as a missing or
/// replaced trust store, all skipped by `--offline` as an empty cache.
fn failure(outcomes: &[Outcome], offline: bool) -> Error {
    let failed =
        || outcomes.iter().filter(|o| o.status.attempted()).map(|o| &o.status);
    if offline {
        Error::NothingCached
    } else if failed()
        .all(|s| matches!(s, Status::Running(_) | Status::Downloading))
    {
        Error::Unanswered
    } else if failed()
        .all(|s| matches!(s, Status::Failed(SourceError::Unreachable(_))))
    {
        Error::Offline
    } else if failed()
        .all(|s| matches!(s, Status::Failed(SourceError::Certificate(_))))
    {
        Error::Certificate(crate::http::certificate_hint())
    } else {
        Error::AllFailed
    }
}

/// Per-source timings for `-v`.
fn notes(out: &Out, outcomes: &[Outcome]) {
    for o in outcomes {
        out.note(&format!(
            "{:<22} {:>5} ms  {:>3} results  {}",
            o.source.id,
            o.elapsed.as_millis(),
            o.datasets.len(),
            o.status.label()
        ));
    }
}

/// Sources that failed, and catalogs still downloading. Sources skipped by
/// `--offline` are counted, not listed: there would be dozens. Live sources
/// still running are not failures; the summary names them.
fn warn_failures(outcomes: &[Outcome], renewing: bool, cache: &Cache) {
    let downloading = |o: &&Outcome| matches!(o.status, Status::Downloading);
    let failed: Vec<String> = outcomes
        .iter()
        .filter(|o| o.status.attempted() && !o.status.answered())
        .filter(|o| !matches!(o.status, Status::Failed(SourceError::Offline)))
        .filter(|o| {
            !matches!(o.status, Status::Running(_) | Status::Downloading)
        })
        .map(|o| format!("{} ({})", o.source.id, o.status.label()))
        .collect();
    if !failed.is_empty() {
        ui::warn(format!("no answer from {}", failed.join(", ")));
    }
    let skipped = outcomes
        .iter()
        .filter(|o| matches!(o.status, Status::Failed(SourceError::Offline)))
        .count();
    if skipped > 0 {
        ui::warn(format!(
            "{} had nothing cached (--offline)",
            ui::count(skipped, "source")
        ));
    }
    let catalogs: Vec<&str> =
        outcomes.iter().filter(downloading).map(|o| o.source.id).collect();
    if !catalogs.is_empty() {
        ui::warn(still_downloading(&catalogs, renewing));
    }
    warn_download_failures(cache, outcomes);
}

fn still_downloading(ids: &[&str], renewing: bool) -> String {
    let (verb, whose, them) = if ids.len() == 1 {
        ("was", "its catalog", "it")
    } else {
        ("were", "their catalogs", "them")
    };
    let next = if renewing {
        "the download goes on in the background for the next search".to_owned()
    } else {
        format!("`dataseek cache warm` fetches {them} once")
    };
    format!("{} {verb} still downloading {whose}; {next}", named(ids))
}

/// The ids, or past [`NAMED`] of them, their count; a first search can leave
/// ten catalogs downloading, and `-v` names each.
fn named(ids: &[&str]) -> String {
    if ids.len() > NAMED {
        ui::count(ids.len(), "source")
    } else {
        ids.join(", ")
    }
}

const NAMED: usize = 3;

/// The closing lines on stderr, after the results: what failed, then one
/// line on how it went and how to see more.
fn summarize(
    query: &str,
    shown: usize,
    found: usize,
    outcomes: &[Outcome],
    renewing: bool,
    cache: &Cache,
) {
    warn_failures(outcomes, renewing, cache);
    let answered = outcomes.iter().filter(|o| o.status.answered()).count();
    let attempted = outcomes.iter().filter(|o| o.status.attempted()).count();
    let running: Vec<&str> = outcomes
        .iter()
        .filter(|o| !o.source.is_catalog())
        .filter(|o| matches!(o.status, Status::Running(_)))
        .map(|o| o.source.id)
        .collect();
    let still = if running.is_empty() {
        String::new()
    } else {
        format!(", {} still running", named(&running))
    };
    let sources = format!(
        "{answered} of {} answered{still}",
        ui::count(attempted, "source")
    );
    if found == 0 {
        ui::warn(format!("no datasets matched \"{query}\"; {sources}"));
    } else if shown < found {
        ui::ok(format!(
            "{shown} of {}; {sources}; -n {found} shows all",
            ui::count(found, "result")
        ));
    } else {
        ui::ok(format!("{}; {sources}", ui::count(found, "result")));
    }
}

/// Tag of the `--json` shape; bumped when a key changes meaning or goes.
const SCHEMA: &str = "dataseek-search/1";

#[derive(Serialize)]
struct Report<'a> {
    schema: &'static str,
    query: &'a str,
    results: &'a [Hit],
    sources: Vec<SourceReport>,
}

#[derive(Serialize)]
struct SourceReport {
    id: &'static str,
    status: String,
    answered: bool,
    results: usize,
    ms: u128,
}

/// What each source did, taken before its results move into the merge.
fn reports(outcomes: &[Outcome]) -> Vec<SourceReport> {
    outcomes
        .iter()
        .map(|o| SourceReport {
            id: o.source.id,
            status: o.status.label(),
            answered: o.status.answered(),
            results: o.datasets.len(),
            ms: o.elapsed.as_millis(),
        })
        .collect()
}

fn print(
    out: &Out,
    query: &str,
    hits: &[Hit],
    sources: Vec<SourceReport>,
) -> Result<()> {
    if out.json {
        return out.json(&Report {
            schema: SCHEMA,
            query,
            results: hits,
            sources,
        });
    }
    let mut w = out.stdout();
    if out.plain {
        for hit in hits {
            let d = &hit.dataset;
            writeln!(
                w,
                "{}\t{}\t{}\t{}\t{}",
                tabless(&d.title),
                d.url,
                hit.sources.join(","),
                d.doi.as_deref().unwrap_or(""),
                d.updated.as_deref().unwrap_or("")
            )?;
        }
        return Ok(());
    }
    let (number, title, muted) =
        (palette::accent(), Style::new().bold(), palette::muted());
    let width = hits.len().to_string().len();
    let indent = width.saturating_add(2);
    let pad = " ".repeat(indent);
    let line = output::width().map(|w| w.saturating_sub(indent));
    for (i, hit) in hits.iter().enumerate() {
        let d = &hit.dataset;
        writeln!(
            w,
            "{number}{:>width$}{number:#}  {title}{}{title:#}",
            i.saturating_add(1),
            fit(&d.title, line)
        )?;
        writeln!(w, "{pad}{}", out.link(&d.url))?;
        writeln!(w, "{pad}{muted}{}{muted:#}", fit(&facts(hit), line))?;
        if let Some(text) = &d.description {
            writeln!(w, "{pad}{}", fit(text, output::prose_width(line)))?;
        }
        writeln!(w)?;
    }
    Ok(())
}

/// The one-line provenance under a title: sources, publisher, date, size,
/// license, whichever are known.
fn facts(hit: &Hit) -> String {
    let d = &hit.dataset;
    let mut parts = vec![hit.sources.join(", ")];
    parts.extend(d.publisher.clone());
    parts.extend(d.updated.clone());
    parts.extend(d.size_bytes.map(human_bytes));
    parts.extend(d.license.clone());
    parts.extend(d.doi.as_ref().map(|doi| format!("doi:{doi}")));
    parts.join(" | ")
}

#[expect(
    clippy::cast_precision_loss,
    reason = "display rounding; exactness past 2^53 bytes is irrelevant"
)]
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len().saturating_sub(1) {
        value /= 1000.0;
        unit = unit.saturating_add(1);
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {name}") }
}

fn tabless(text: &str) -> String {
    text.replace(['\t', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Dataset;
    use crate::sources::SOURCES;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(155_173), "155.2 KB");
        assert_eq!(human_bytes(2_559_248_010_229), "2.6 TB");
    }

    #[test]
    fn only_catalogs_searched_expired_or_still_downloading_are_renewed() {
        let mut catalogs = SOURCES.iter().filter(|s| s.is_catalog());
        let (Some(a), Some(b), Some(c)) =
            (catalogs.next(), catalogs.next(), catalogs.next())
        else {
            panic!("fewer than three catalogs");
        };
        let live = SOURCES.iter().find(|s| !s.is_catalog()).unwrap();
        let outcome = |source, status| Outcome {
            source,
            status,
            elapsed: Duration::ZERO,
            datasets: Vec::new(),
        };
        let outcomes = [
            outcome(a, Status::Downloading),
            outcome(b, Status::Expired),
            outcome(c, Status::Fetched),
            outcome(live, Status::Expired),
            outcome(live, Status::Running(Duration::ZERO)),
        ];
        assert_eq!(to_renew(&outcomes), [a.id, b.id]);
    }

    #[test]
    fn the_downloading_warning_agrees_with_its_count() {
        assert!(
            still_downloading(&["openneuro"], false)
                .starts_with("openneuro was still downloading its catalog;")
        );
        assert!(
            still_downloading(&["openneuro", "physionet"], true).starts_with(
                "openneuro, physionet were still downloading their catalogs;"
            )
        );
        assert!(
            still_downloading(&["a", "b", "c", "d"], true).starts_with(
                "4 sources were still downloading their catalogs;"
            )
        );
    }

    fn failed_with(errors: Vec<SourceError>) -> Vec<Outcome> {
        SOURCES
            .iter()
            .zip(errors)
            .map(|(source, error)| Outcome {
                source,
                status: Status::Failed(error),
                elapsed: Duration::ZERO,
                datasets: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn refused_certificates_everywhere_are_a_trust_store_problem() {
        let refused =
            || SourceError::Certificate("invalid peer certificate".into());
        let error = failure(&failed_with(vec![refused(), refused()]), false);
        assert!(matches!(error, Error::Certificate(_)), "{error}");
        let hint = crate::http::certificate_hint();
        assert!(error.to_string().ends_with(&hint), "{error}");

        let mixed = vec![refused(), SourceError::Status(503)];
        let error = failure(&failed_with(mixed), false);
        assert!(matches!(error, Error::AllFailed), "{error}");
    }

    const PUBLISHER_BYTES: usize = 16_000;
    /// What ranking [`answered`] allocates: keys, the owner map, hits, the
    /// text weighing reads and the term sets rarity weighting builds. One
    /// publisher copied on top of it reaches the bound.
    const RANKING_BYTES: usize = 430_068;

    fn rain(url: &str, publisher: bool) -> Dataset {
        let mut d = Dataset::new("Rain", url);
        d.publisher = publisher.then(|| "p".repeat(PUBLISHER_BYTES));
        d
    }

    /// Four sources answering 50 records each, most carrying a 16 KB
    /// publisher. The first source's first 25 come back from the second,
    /// first without a publisher and then with one, so merging hands it
    /// over. The third source's first 25 lack one too; the fourth lists each
    /// under another link with a publisher, then bridges the two links, so
    /// absorbing one hit into another hands it over. The title is shorter
    /// than the 12 characters a title key needs, so neither merging nor
    /// weighing reads the publisher, and any of its bytes allocated while
    /// ranking is a copy.
    fn answered() -> Vec<Outcome> {
        let url = |path: String| format!("https://x.org/{path}");
        let lists: [Vec<Dataset>; 4] = [
            (0..50)
                .map(|i| match i {
                    0..25 => rain(&url(format!("a/{i}")), false),
                    _ => rain(&url(format!("0/{i}")), true),
                })
                .collect(),
            (0..50)
                .map(|i| match i {
                    0..25 => rain(&url(format!("a/{i}")), true),
                    _ => rain(&url(format!("1/{i}")), true),
                })
                .collect(),
            (0..50)
                .map(|i| match i {
                    0..25 => rain(&url(format!("b/{i}")), false),
                    _ => rain(&url(format!("2/{i}")), true),
                })
                .collect(),
            (0..25)
                .map(|i| rain(&url(format!("c/{i}")), true))
                .chain((0..25).map(|i| {
                    let mut bridge = rain(&url(format!("b/{i}")), false);
                    bridge.aliases.push(url(format!("c/{i}")));
                    bridge
                }))
                .collect(),
        ];
        SOURCES
            .iter()
            .zip(lists)
            .map(|(source, datasets)| Outcome {
                source,
                status: Status::Fetched,
                elapsed: Duration::ZERO,
                datasets,
            })
            .collect()
    }

    #[test]
    fn ranking_moves_the_records_instead_of_copying_them() {
        let mut outcomes = answered();
        let mut hits = Vec::new();
        let allocated = allocation_counter::measure(|| {
            hits = ranked(&mut outcomes, "rain", Sort::Relevance);
        })
        .bytes_total;
        assert_eq!(hits.len(), 125);
        assert_eq!(hits.iter().filter(|h| h.sources.len() == 2).count(), 50);
        assert!(hits.iter().all(|h| {
            h.dataset.publisher.as_ref().map(String::len)
                == Some(PUBLISHER_BYTES)
        }));
        let bound = RANKING_BYTES.saturating_add(PUBLISHER_BYTES) as u64;
        assert!(
            allocated < bound,
            "ranking allocated {allocated} bytes, {RANKING_BYTES} expected"
        );
    }
}
