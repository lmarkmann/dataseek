//! Recording the snapshot: every default source asked every query, live,
//! the way `dataseek search` asks them, with no key and an empty cache.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use dataseek::internals::{Outcome, Plan, Services, search, select};
use indicatif::ProgressBar;

use crate::snapshot::{self, Answered, Lists, Query};

/// Results asked of each source: the `--per-source` default.
const PER_SOURCE: usize = 10;

/// A pause before asking the failed sources once more, so a source that
/// rate-limited the first request has room to answer the second.
const RETRY_AFTER: Duration = Duration::from_secs(5);

pub fn run(queries: &[Query], only: &[String]) -> Result<()> {
    let scratch = tempfile::tempdir()?;
    let services = Arc::new(Services::scratch(scratch.path()));
    // Sources whose terms bar storing their results are never written down.
    let sources: Vec<_> =
        select(&[], &[], &[]).into_iter().filter(|s| s.persist).collect();
    let chosen: Vec<&Query> = queries
        .iter()
        .filter(|q| only.is_empty() || only.contains(&q.id))
        .collect();
    let mut err = std::io::stderr().lock();
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
        let (answered, lists) = freeze(&outcomes);
        snapshot::save(query, &answered, &lists)?;
        let records: usize = lists.iter().map(|(_, ds)| ds.len()).sum();
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
    Ok(())
}

fn freeze(outcomes: &[Outcome]) -> (Vec<Answered>, Lists) {
    let answered = outcomes
        .iter()
        .map(|o| Answered {
            id: o.source.id.to_owned(),
            status: o.status.label(),
            results: o.datasets.len(),
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
