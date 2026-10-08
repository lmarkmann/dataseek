//! The search loop: every chosen source runs on its own thread, and each one
//! independently walks the same states, so one slow or broken source can
//! cost time but never results.
//!
//! A catalog with no copy on disk downloads during the search, and some take
//! longer than the deadline (OpenNeuro pages its list 100 at a time, about 40
//! seconds in all). With a deadline set, the loop stops waiting for those once
//! every other source has answered; the caller finishes them in the
//! background.
//!
//! Per source, in order: skipped when a required key is missing; skipped for
//! a few minutes after an outage unless the user named it; served from the
//! query cache when fresh (unless `--refresh`); otherwise fetched. A fetch
//! that fails falls back to an expired cache entry when one exists, and an
//! outage-class failure marks the source so the next searches skip it. A
//! search also owns a deadline: once it passes, the loop stops waiting and
//! sets [`Services::stop`], every adapter stops before its next page, and
//! the failure it reports is never marked as an outage, because slow is
//! not down. The loop returns one [`Outcome`] per source in registry
//! order; merging and printing are the caller's.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use indicatif::ProgressBar;

use crate::cache::{Freshness, Kind, QUERY_TTL, query_key};
use crate::credentials::Key;
use crate::http::{self, SourceError};
use crate::record::Dataset;
use crate::sources::{Answer, Ctx, Services, Source};

pub struct Plan {
    pub query: String,
    pub sources: Vec<&'static Source>,
    pub per_source: usize,
    /// The user named these sources, or `--offline` costs no request, so
    /// recent outages do not skip them.
    pub forced: bool,
}

#[derive(Debug)]
pub enum Status {
    Fetched,
    Cached,
    /// The fetch failed; an expired cache entry was served instead.
    Stale(SourceError),
    /// An expired catalog was searched; it wants downloading again.
    Expired,
    Failed(SourceError),
    NeedsKey(Key),
    /// Skipped because the source had an outage this long ago.
    Resting(Duration),
    /// Still working when the search deadline passed.
    Running(Duration),
}

impl Status {
    pub fn answered(&self) -> bool {
        matches!(
            self,
            Self::Fetched | Self::Cached | Self::Stale(_) | Self::Expired
        )
    }

    pub fn attempted(&self) -> bool {
        !matches!(self, Self::NeedsKey(_) | Self::Resting(_))
    }

    pub fn label(&self) -> String {
        match self {
            Self::Fetched => "ok".to_owned(),
            Self::Cached => "cached".to_owned(),
            Self::Stale(e) => format!("stale cache ({e})"),
            Self::Expired => "expired catalog".to_owned(),
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
    let first_download: Vec<bool> = plan
        .sources
        .iter()
        .map(|s| s.is_catalog() && !services.cache.has(Kind::Catalog, s.id))
        .collect();
    let mut others = first_download.iter().filter(|&&first| !first).count();
    let waits_for_downloads = deadline.is_none() || others == 0;
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
            if !matches!(outcome.status, Status::NeedsKey(_)) {
                progress.inc(1);
            }
            let _ = sender.send((index, outcome));
        });
    }
    drop(sender);

    let mut slots: Vec<Option<Outcome>> =
        plan.sources.iter().map(|_| None).collect();
    let mut waiting = plan.sources.len();
    let mut gave_up = false;
    while waiting > 0 {
        if others == 0 && !waits_for_downloads {
            gave_up = true;
            break;
        }
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
                    if first_download.get(index) == Some(&false) {
                        others = others.saturating_sub(1);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                gave_up = true;
                services.stop.store(true, Ordering::Relaxed);
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
                status: if gave_up {
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
        Ok(Answer { datasets, expired: true }) => {
            done(Status::Expired, datasets)
        }
        Ok(Answer { datasets, expired: false }) => {
            if !http::is_offline() {
                ctx.cache.clear_outage(source.id);
            }
            if cacheable {
                ctx.cache.store(Kind::Query, &key, &datasets);
            }
            done(Status::Fetched, datasets)
        }
        Err(error) => {
            // A source the deadline cut off is slow, not down: its next
            // page may be another host's fault, or the client's own limit.
            if error.is_outage() && !ctx.stopped() {
                ctx.cache.mark_outage(source.id);
            }
            match cached {
                Some((datasets, _)) => done(Status::Stale(error), datasets),
                None => done(Status::Failed(error), Vec::new()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{Adapter, Category, Need};

    type Answer = Result<Vec<Dataset>, SourceError>;

    fn found() -> Vec<Dataset> {
        vec![Dataset::new("Rainfall", "https://x.org/rain")]
    }

    #[expect(clippy::unnecessary_wraps, reason = "the Live signature")]
    fn answers(_: &Ctx<'_>, _: &str, _: usize) -> Answer {
        Ok(found())
    }

    fn times_out(_: &Ctx<'_>, _: &str, _: usize) -> Answer {
        Err(SourceError::Timeout)
    }

    fn not_found(_: &Ctx<'_>, _: &str, _: usize) -> Answer {
        Err(SourceError::Status(404))
    }

    fn list_times_out(_: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
        Err(SourceError::Timeout)
    }

    #[expect(clippy::unnecessary_wraps, reason = "the Listing signature")]
    fn list_hangs(_: &Ctx<'_>) -> Result<Vec<Dataset>, SourceError> {
        std::thread::sleep(Duration::from_secs(5));
        Ok(found())
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "being called at all is the failure"
    )]
    fn must_not_run(_: &Ctx<'_>, _: &str, _: usize) -> Answer {
        panic!("the adapter ran when it should have been skipped");
    }

    #[expect(clippy::unnecessary_wraps, reason = "the Live signature")]
    fn hangs(_: &Ctx<'_>, _: &str, _: usize) -> Answer {
        std::thread::sleep(Duration::from_secs(5));
        Ok(Vec::new())
    }

    const fn fake(id: &'static str, run: crate::sources::Live) -> Source {
        Source {
            id,
            name: id,
            category: Category::Research,
            protocol: "test",
            docs: "https://x.org",
            key: None,
            persist: true,
            opt_in: None,
            notice: None,
            adapter: Adapter::Live(run),
        }
    }

    static ANSWERS: Source = fake("answers", answers);
    static TIMES_OUT: Source = fake("times-out", times_out);
    static NOT_FOUND: Source = fake("not-found", not_found);
    static UNTOUCHABLE: Source = fake("untouchable", must_not_run);
    static KEYED: Source = Source {
        key: Some((Key::Roboflow, Need::Required)),
        ..fake("keyed", must_not_run)
    };
    static UNPERSISTED: Source =
        Source { persist: false, ..fake("unpersisted", answers) };
    static HANGS: Source = fake("hangs", hangs);
    static CATALOG_DOWN: Source = Source {
        adapter: Adapter::Catalog(list_times_out),
        ..fake("catalog-down", answers)
    };
    static CATALOG_HANGS: Source = Source {
        adapter: Adapter::Catalog(list_hangs),
        ..fake("catalog-hangs", answers)
    };

    fn plan(source: &'static Source, forced: bool) -> Plan {
        Plan {
            query: "rain".into(),
            sources: vec![source],
            per_source: 10,
            forced,
        }
    }

    fn query_entry(source: &Source) -> String {
        query_key(source.id, "rain", 10)
    }

    #[test]
    fn a_missing_required_key_skips_the_source_unasked() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let outcome = one(&services.ctx(false), &plan(&KEYED, true), &KEYED);
        assert!(matches!(outcome.status, Status::NeedsKey(Key::Roboflow)));
    }

    #[test]
    fn a_recent_outage_rests_the_source_unless_the_user_named_it() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let ctx = services.ctx(false);
        ctx.cache.mark_outage(UNTOUCHABLE.id);
        let rested = one(&ctx, &plan(&UNTOUCHABLE, false), &UNTOUCHABLE);
        assert!(
            matches!(rested.status, Status::Resting(_)),
            "{:?}",
            rested.status
        );

        ctx.cache.mark_outage(ANSWERS.id);
        let named = one(&ctx, &plan(&ANSWERS, true), &ANSWERS);
        assert!(matches!(named.status, Status::Fetched), "{:?}", named.status);
        assert!(
            ctx.cache.recent_outage(ANSWERS.id).is_none(),
            "a success left the outage mark"
        );
    }

    #[test]
    fn a_fresh_answer_is_served_from_cache_unless_refreshing() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let cached = vec![Dataset::new("Snow", "https://x.org/snow")];
        let ctx = services.ctx(false);
        ctx.cache.store(Kind::Query, &query_entry(&UNTOUCHABLE), &cached);
        let served = one(&ctx, &plan(&UNTOUCHABLE, true), &UNTOUCHABLE);
        assert!(
            matches!(served.status, Status::Cached),
            "{:?}",
            served.status
        );
        assert_eq!(served.datasets, cached);

        ctx.cache.store(Kind::Query, &query_entry(&ANSWERS), &cached);
        let refreshed =
            one(&services.ctx(true), &plan(&ANSWERS, true), &ANSWERS);
        assert!(matches!(refreshed.status, Status::Fetched));
        assert_eq!(refreshed.datasets, found());
    }

    #[test]
    fn a_failed_fetch_serves_the_expired_answer() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let ctx = services.ctx(false);
        ctx.cache.store_expired(
            Kind::Query,
            &query_entry(&TIMES_OUT),
            &found(),
        );
        let outcome = one(&ctx, &plan(&TIMES_OUT, true), &TIMES_OUT);
        assert!(
            matches!(outcome.status, Status::Stale(SourceError::Timeout)),
            "{:?}",
            outcome.status
        );
        assert_eq!(outcome.datasets, found());
    }

    #[test]
    fn only_outage_failures_rest_the_source_next_time() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let ctx = services.ctx(false);
        let down = one(&ctx, &plan(&TIMES_OUT, true), &TIMES_OUT);
        assert!(matches!(down.status, Status::Failed(SourceError::Timeout)));
        assert!(ctx.cache.recent_outage(TIMES_OUT.id).is_some());

        let rejected = one(&ctx, &plan(&NOT_FOUND, true), &NOT_FOUND);
        assert!(matches!(
            rejected.status,
            Status::Failed(SourceError::Status(404))
        ));
        assert!(ctx.cache.recent_outage(NOT_FOUND.id).is_none());
    }

    #[test]
    fn an_unpersisted_source_writes_nothing_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let ctx = services.ctx(false);
        let outcome = one(&ctx, &plan(&UNPERSISTED, true), &UNPERSISTED);
        assert_eq!(outcome.datasets, found());
        assert_eq!(ctx.cache.usage().files, 0);
    }

    #[test]
    fn a_catalog_searched_from_an_expired_copy_reports_expired() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let ctx = services.ctx(false);
        ctx.cache.store_expired(Kind::Catalog, CATALOG_DOWN.id, &found());
        let outcome = one(&ctx, &plan(&CATALOG_DOWN, true), &CATALOG_DOWN);
        assert!(
            matches!(outcome.status, Status::Expired),
            "{:?}",
            outcome.status
        );
        assert!(outcome.status.answered());
        assert_eq!(outcome.datasets, found());
    }

    #[test]
    fn an_offline_search_never_clears_an_outage_mark() {
        crate::http::go_offline();
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        let ctx = services.ctx(false);
        ctx.cache.store(Kind::Catalog, CATALOG_DOWN.id, &found());
        ctx.cache.mark_outage(CATALOG_DOWN.id);
        let outcome = one(&ctx, &plan(&CATALOG_DOWN, true), &CATALOG_DOWN);
        assert!(outcome.status.answered(), "{:?}", outcome.status);
        assert!(
            ctx.cache.recent_outage(CATALOG_DOWN.id).is_some(),
            "an offline run cleared the mark"
        );
    }

    fn run_one(
        source: &'static Source,
        deadline: Option<Duration>,
    ) -> Outcome {
        let dir = tempfile::tempdir().unwrap();
        let services = Arc::new(Services::scratch(dir.path()));
        let plan = Arc::new(plan(source, true));
        let mut outcomes =
            run(&services, false, &plan, &ProgressBar::hidden(), deadline);
        assert_eq!(outcomes.len(), 1);
        outcomes.remove(0)
    }

    #[test]
    fn a_panicking_adapter_is_reported_as_crashed() {
        let outcome = run_one(&UNTOUCHABLE, None);
        assert_eq!(
            outcome.status.label(),
            "returned an unexpected shape: the adapter crashed"
        );
        assert!(!outcome.status.answered());
    }

    #[test]
    fn a_first_catalog_download_is_not_waited_for_once_the_rest_answered() {
        let dir = tempfile::tempdir().unwrap();
        let services = Arc::new(Services::scratch(dir.path()));
        let plan = Arc::new(Plan {
            sources: vec![&ANSWERS, &CATALOG_HANGS],
            ..plan(&ANSWERS, true)
        });
        let started = Instant::now();
        let outcomes = run(
            &services,
            false,
            &plan,
            &ProgressBar::hidden(),
            Some(Duration::from_secs(20)),
        );
        assert!(started.elapsed() < Duration::from_secs(4));
        let statuses: Vec<_> = outcomes.iter().map(|o| &o.status).collect();
        assert!(
            matches!(statuses[..], [Status::Fetched, Status::Running(_)]),
            "{statuses:?}"
        );
    }

    #[test]
    fn an_adapter_past_the_deadline_is_still_running() {
        let outcome = run_one(&HANGS, Some(Duration::from_millis(50)));
        assert!(
            matches!(outcome.status, Status::Running(_)),
            "{:?}",
            outcome.status
        );
    }

    #[test]
    fn the_passing_deadline_sets_the_stop_flag() {
        let dir = tempfile::tempdir().unwrap();
        let services = Arc::new(Services::scratch(dir.path()));
        let plan = Arc::new(plan(&HANGS, true));
        let outcomes = run(
            &services,
            false,
            &plan,
            &ProgressBar::hidden(),
            Some(Duration::from_millis(50)),
        );
        assert!(
            services.stop.load(Ordering::Relaxed),
            "a passing deadline left the sources paging"
        );
        assert!(matches!(
            outcomes.first().unwrap().status,
            Status::Running(_)
        ));
    }

    #[test]
    fn a_failure_past_the_deadline_parks_no_source() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        services.stop.store(true, Ordering::Relaxed);
        let ctx = services.ctx(false);
        let outcome = one(&ctx, &plan(&TIMES_OUT, true), &TIMES_OUT);
        assert!(matches!(
            outcome.status,
            Status::Failed(SourceError::Timeout)
        ));
        assert!(
            ctx.cache.recent_outage(TIMES_OUT.id).is_none(),
            "a deadline-cut-off source was parked as down"
        );
    }

    #[test]
    fn a_complete_answer_is_kept_even_past_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let services = Services::scratch(dir.path());
        services.stop.store(true, Ordering::Relaxed);
        let ctx = services.ctx(false);
        let outcome = one(&ctx, &plan(&ANSWERS, true), &ANSWERS);
        assert!(matches!(outcome.status, Status::Fetched));
        assert_eq!(outcome.datasets, found());
        assert_eq!(ctx.cache.usage().files, 1, "the full answer was not kept");
    }
}
