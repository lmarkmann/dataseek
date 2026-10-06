//! The search loop: every chosen source runs on its own thread, and each one
//! independently walks the same states, so one slow or broken source can
//! cost time but never results.
//!
//! Per source, in order: skipped when a required key is missing; skipped for
//! a few minutes after an outage unless the user named it; served from the
//! query cache when fresh (unless `--refresh`); otherwise fetched. A fetch
//! that fails falls back to an expired cache entry when one exists, and an
//! outage-class failure marks the source so the next searches skip it. The
//! loop returns one [`Outcome`] per source in registry order; merging and
//! printing are the caller's.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use indicatif::ProgressBar;

use crate::cache::{Freshness, Kind, QUERY_TTL, query_key};
use crate::credentials::Key;
use crate::http::SourceError;
use crate::record::Dataset;
use crate::sources::{Ctx, Services, Source};

pub struct Plan {
    pub query: String,
    pub sources: Vec<&'static Source>,
    pub per_source: usize,
    /// The user named these sources, so recent outages do not skip them.
    pub forced: bool,
}

#[derive(Debug)]
pub enum Status {
    Fetched,
    Cached,
    /// The fetch failed; an expired cache entry was served instead.
    Stale(SourceError),
    Failed(SourceError),
    NeedsKey(Key),
    /// Skipped because the source had an outage this long ago.
    Resting(Duration),
    /// Still working when the search deadline passed.
    Running(Duration),
}

impl Status {
    pub fn answered(&self) -> bool {
        matches!(self, Self::Fetched | Self::Cached | Self::Stale(_))
    }

    pub fn attempted(&self) -> bool {
        !matches!(self, Self::NeedsKey(_) | Self::Resting(_))
    }

    pub fn label(&self) -> String {
        match self {
            Self::Fetched => "ok".to_owned(),
            Self::Cached => "cached".to_owned(),
            Self::Stale(e) => format!("stale cache ({e})"),
            Self::Failed(e) => e.to_string(),
            Self::NeedsKey(key) => format!("needs ${}", key.env_var()),
            Self::Resting(ago) => {
                format!("skipped, outage {} s ago", ago.as_secs())
            }
            Self::Running(after) => {
                format!("still running after {} s", after.as_secs())
            }
        }
    }
}

pub struct Outcome {
    pub source: &'static Source,
    pub status: Status,
    pub elapsed: Duration,
    pub datasets: Vec<Dataset>,
}

pub fn run(
    services: &Arc<Services>,
    refresh: bool,
    plan: &Arc<Plan>,
    progress: &ProgressBar,
    deadline: Option<Duration>,
) -> Vec<Outcome> {
    let started = Instant::now();
    let (sender, receiver) = mpsc::channel();
    for (index, &source) in plan.sources.iter().enumerate() {
        let (services, plan, progress, sender) = (
            Arc::clone(services),
            Arc::clone(plan),
            progress.clone(),
            sender.clone(),
        );
        std::thread::spawn(move || {
            let outcome = one(&services.ctx(refresh), &plan, source);
            progress.inc(1);
            let _ = sender.send((index, outcome));
        });
    }
    drop(sender);

    let mut slots: Vec<Option<Outcome>> =
        plan.sources.iter().map(|_| None).collect();
    let mut waiting = plan.sources.len();
    let mut deadline_passed = false;
    while waiting > 0 {
        let next = match deadline {
            None => {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            }
            Some(limit) => match limit.checked_sub(started.elapsed()) {
                Some(left) => receiver.recv_timeout(left),
                None => Err(RecvTimeoutError::Timeout),
            },
        };
        match next {
            Ok((index, outcome)) => {
                if let Some(slot) = slots.get_mut(index) {
                    *slot = Some(outcome);
                    waiting = waiting.saturating_sub(1);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                deadline_passed = true;
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    slots
        .into_iter()
        .zip(&plan.sources)
        .map(|(slot, &source)| {
            slot.unwrap_or_else(|| Outcome {
                source,
                status: if deadline_passed {
                    Status::Running(started.elapsed())
                } else {
                    Status::Failed(SourceError::shape("the adapter crashed"))
                },
                elapsed: started.elapsed(),
                datasets: Vec::new(),
            })
        })
        .collect()
}

fn one(ctx: &Ctx<'_>, plan: &Plan, source: &'static Source) -> Outcome {
    let started = Instant::now();
    let done = |status: Status, datasets: Vec<Dataset>| Outcome {
        source,
        status,
        elapsed: started.elapsed(),
        datasets,
    };

    if let Some(key) = source.missing_key(ctx.creds) {
        return done(Status::NeedsKey(key), Vec::new());
    }
    if !plan.forced
        && let Some(ago) = ctx.cache.recent_outage(source.id)
    {
        return done(Status::Resting(ago), Vec::new());
    }

    let cacheable = source.persist && !source.is_catalog();
    let key = query_key(source.id, &plan.query, plan.per_source);
    let cached = if cacheable {
        ctx.cache.load::<Vec<Dataset>>(Kind::Query, &key, QUERY_TTL)
    } else {
        None
    };
    if !ctx.refresh
        && let Some((datasets, Freshness::Fresh)) = &cached
    {
        return done(Status::Cached, datasets.clone());
    }

    match source.search(ctx, &plan.query, plan.per_source) {
        Ok(datasets) => {
            ctx.cache.clear_outage(source.id);
            if cacheable {
                ctx.cache.store(Kind::Query, &key, &datasets);
            }
            done(Status::Fetched, datasets)
        }
        Err(error) => {
            if error.is_outage() {
                ctx.cache.mark_outage(source.id);
            }
            match cached {
                Some((datasets, _)) => done(Status::Stale(error), datasets),
                None => done(Status::Failed(error), Vec::new()),
            }
        }
    }
}
