//! Recording the snapshot: every default source asked every query, live,
//! the way `dataseek search` asks them, with no key and an empty cache.
//! `--source <id>` asks that one source and splices its lists into the
//! snapshot, so an adapter change is measured without every other source's
//! lists moving too.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use dataseek::internals::{
    Outcome, Plan, SOURCES, Services, Source, search, select,
};
use indicatif::ProgressBar;

use crate::snapshot::{self, Answered, Lists, Query};

/// Results asked of each source: the `--per-source` default.
const PER_SOURCE: usize = 10;

/// A pause before asking the failed sources once more, so a source that
/// rate-limited the first request has room to answer the second.
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// Re-records the queries named in `args`, or all of them. A query on which
/// a source that answered last time fails now keeps its old snapshot, and
/// the run fails at the end, unless `--accept-lost` is given.
pub fn run(queries: &[Query], args: &[String]) -> Result<()> {
    let accept_lost = args.iter().any(|a| a == "--accept-lost");
    let (spliced, args) = source_flag(args)?;
    let only: Vec<String> =
        args.into_iter().filter(|a| a != "--accept-lost").collect();
    let chosen = pick(queries, &only)?;
    let scratch = tempfile::tempdir()?;
    let services = Arc::new(Services::scratch(scratch.path()));
    // Sources whose terms bar storing their results are never written down.
    let sources: Vec<_> = match spliced {
        Some(source) => vec![source],
        None => {
            select(&[], &[], &[]).into_iter().filter(|s| s.persist).collect()
        }
    };
    let mut err = std::io::stderr().lock();
    let mut kept = Vec::new();
    let mut ready = Vec::new();
    for (i, query) in chosen.iter().enumerate() {
        let ask = |sources| {
            let plan = Arc::new(Plan {
                query: query.text.clone(),
                sources,
                per_source: PER_SOURCE,
                forced: true,
            });
            search(&services, false, &plan, &ProgressBar::hidden(), None)
        };
        let mut outcomes = ask(sources.clone());
        let failed: Vec<_> = outcomes
            .iter()
            .filter(|o| o.status.attempted() && !o.status.answered())
            .map(|o| o.source)
            .collect();
        if !failed.is_empty() {
            std::thread::sleep(RETRY_AFTER);
            for retried in ask(failed) {
                if let Some(slot) = outcomes
                    .iter_mut()
                    .find(|o| o.source.id == retried.source.id)
                {
                    *slot = retried;
                }
            }
        }
        if spliced.is_some() {
            match splice(query, &outcomes, accept_lost)? {
                Ok(retrieval) => ready.push((*query, retrieval)),
                Err(lost) => kept.push(lost),
            }
        } else {
            kept.extend(store(query, &outcomes, accept_lost)?);
        }
        let records: usize = outcomes.iter().map(|o| o.datasets.len()).sum();
        let ok = outcomes.iter().filter(|o| o.status.answered()).count();
        writeln!(
            err,
            "{:>2}/{} {:<28} {ok:>2} of {} sources answered, {records} records",
            i.saturating_add(1),
            chosen.len(),
            query.id,
            outcomes.len()
        )?;
    }
    if !kept.is_empty() && spliced.is_some() {
        bail!(
            "sources that answered last time failed, so no snapshot was changed:\n  {}\n  Try:   re-record later, or pass --accept-lost to record the failures",
            kept.join("\n  ")
        );
    }
    for (query, retrieval) in &ready {
        snapshot::save(
            query,
            &retrieval.dataseek,
            &retrieval.sources,
            &retrieval.lists,
        )?;
    }
    if !kept.is_empty() {
        bail!(
            "sources that answered last time failed, so these snapshots were kept:\n  {}\n  Try:   re-record them later, or pass --accept-lost to record the failures",
            kept.join("\n  ")
        );
    }
    Ok(())
}

/// Saves the recording of `query`, unless a source that answered last time
/// failed now and `accept_lost` is off; then the old snapshot stays and the
/// lost sources are returned.
fn store(
    query: &Query,
    outcomes: &[Outcome],
    accept_lost: bool,
) -> Result<Option<String>> {
    let (answered, lists) = freeze(outcomes);
    let gone = lost(&snapshot::recorded(query)?, &answered);
    if !gone.is_empty() && !accept_lost {
        return Ok(Some(format!("{}: {}", query.id, gone.join(", "))));
    }
    snapshot::save(query, env!("CARGO_PKG_VERSION"), &answered, &lists)?;
    Ok(None)
}

/// The source `--source <id>` names, and the arguments without the flag. A
/// source whose terms bar storing its results is refused, as in a full
/// recording.
fn source_flag(
    args: &[String],
) -> Result<(Option<&'static Source>, Vec<String>)> {
    let Some(at) = args.iter().position(|a| a == "--source") else {
        return Ok((None, args.to_vec()));
    };
    let Some(id) = args.get(at.saturating_add(1)) else {
        bail!(
            "--source needs a source id\n  Try:   just relevance record --source huggingface"
        );
    };
    let Some(source) = SOURCES.iter().find(|s| s.id == id) else {
        bail!(
            "no source has the id {id:?}\n  Try:   an id from `dataseek sources`"
        );
    };
    if !source.persist {
        bail!("{id}'s terms bar storing its results, so it is never recorded");
    }
    let rest = args
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != at && *i != at.saturating_add(1))
        .map(|(_, a)| a.clone())
        .collect();
    Ok((Some(source), rest))
}

/// The recorded snapshot of `query` with one source's list and status
/// replaced, every other source's kept, ready to save; or, as [`store`]
/// decides, the lost sources. Nothing is written here, so a run that stops
/// partway leaves every snapshot as it was.
fn splice(
    query: &Query,
    outcomes: &[Outcome],
    accept_lost: bool,
) -> Result<Result<snapshot::Retrieval, String>> {
    let (answered, lists) = freeze(outcomes);
    let mut recorded = snapshot::load(query)?;
    let gone = lost(&recorded.sources, &answered);
    if !gone.is_empty() && !accept_lost {
        return Ok(Err(format!("{}: {}", query.id, gone.join(", "))));
    }
    for (mut status, (id, list)) in answered.into_iter().zip(lists) {
        status.recorded_with = Some(env!("CARGO_PKG_VERSION").to_owned());
        if let Some(i) =
            recorded.sources.iter().position(|s| s.id == status.id)
        {
            if let Some(slot) = recorded.sources.get_mut(i) {
                *slot = status;
            }
            if let Some(slot) = recorded.lists.get_mut(i) {
                *slot = (id, list);
            }
        } else {
            recorded.sources.push(status);
            recorded.lists.push((id, list));
        }
    }
    Ok(Ok(recorded))
}

/// The queries named in `only`, or all of them; an unknown id is an error.
fn pick<'a>(queries: &'a [Query], only: &[String]) -> Result<Vec<&'a Query>> {
    if let Some(unknown) =
        only.iter().find(|id| !queries.iter().any(|q| &q.id == *id))
    {
        bail!(
            "no query has the id {unknown:?}\n  Try:   an id from tests/fixtures/relevance/queries.toml"
        );
    }
    Ok(queries
        .iter()
        .filter(|q| only.is_empty() || only.contains(&q.id))
        .collect())
}

/// Sources that answered in `before` and did not answer in `now`, each with
/// what it said this time.
fn lost(before: &[Answered], now: &[Answered]) -> Vec<String> {
    now.iter()
        .filter(|s| !s.answered())
        .filter(|s| before.iter().any(|b| b.id == s.id && b.answered()))
        .map(|s| format!("{} ({})", s.id, s.status))
        .collect()
}

fn freeze(outcomes: &[Outcome]) -> (Vec<Answered>, Lists) {
    let answered = outcomes
        .iter()
        .map(|o| Answered {
            id: o.source.id.to_owned(),
            status: o.status.label(),
            results: o.datasets.len(),
            recorded_with: None,
        })
        .collect();
    let lists = outcomes
        .iter()
        .map(|o| {
            let frozen =
                o.datasets.iter().cloned().map(snapshot::frozen).collect();
            (o.source.id, frozen)
        })
        .collect();
    (answered, lists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{Half, Kind};

    fn answered(id: &str, status: &str) -> Answered {
        Answered {
            id: id.into(),
            status: status.into(),
            results: 0,
            recorded_with: None,
        }
    }

    #[test]
    fn a_source_that_answered_before_and_fails_now_is_lost() {
        let before = [
            answered("a", "ok"),
            answered("b", "ok"),
            answered("c", "answered HTTP 500"),
        ];
        let now = [
            answered("a", "answered HTTP 400"),
            answered("b", "ok"),
            answered("c", "answered HTTP 500"),
            answered("d", "timed out"),
        ];
        assert_eq!(lost(&before, &now), ["a (answered HTTP 400)"]);
    }

    #[test]
    fn the_source_flag_names_one_storable_source() {
        let args = |a: &[&str]| -> Vec<String> {
            a.iter().map(|s| (*s).to_owned()).collect()
        };
        let (source, rest) =
            source_flag(&args(&["iris", "--source", "huggingface"])).unwrap();
        assert_eq!(source.map(|s| s.id), Some("huggingface"));
        assert_eq!(rest, ["iris"]);
        assert!(source_flag(&args(&["iris"])).unwrap().0.is_none());
        assert!(source_flag(&args(&["--source"])).is_err());
        assert!(source_flag(&args(&["--source", "nope"])).is_err());
        assert!(source_flag(&args(&["--source", "kaggle"])).is_err());
    }

    #[test]
    fn an_unknown_query_id_is_refused_before_anything_is_asked() {
        let query = Query {
            id: "iris".into(),
            text: "iris".into(),
            kind: Kind::Known,
            half: Half::Held,
            targets: Vec::new(),
        };
        let queries = [query];
        assert_eq!(pick(&queries, &[]).unwrap().len(), 1);
        assert!(pick(&queries, &["iris".into()]).is_ok());
        assert!(pick(&queries, &["irs".into()]).is_err());
    }
}
