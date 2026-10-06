//! `cache info`, `cache warm` and `cache clear`.

use std::io::Write;

use anyhow::{Context, Result};

use crate::cache::{BUDGET_BYTES, BUDGET_FILES, Cache};
use crate::cli::CacheAction;
use crate::find::human_bytes;
use crate::http::SourceError;
use crate::output::Out;
use crate::sources::{SOURCES, Services};
use crate::{paths, ui};

pub fn run(action: CacheAction, out: &Out) -> Result<()> {
    let cache = Cache::new(paths::resolve()?.cache);
    match action {
        CacheAction::Info => info(&cache, out),
        CacheAction::Warm => warm(out),
        CacheAction::Clear => {
            cache.clear().with_context(|| {
                format!("cannot remove {}", cache.root().display())
            })?;
            ui::ok(format!("cleared {}", cache.root().display()));
            Ok(())
        }
    }
}

/// Every catalog source downloads in parallel, with no deadline; the result
/// per source goes to stdout so a failed one is visible in a pipe too.
fn warm(out: &Out) -> Result<()> {
    let services = Services::load()?;
    let ctx = services.ctx(true);
    let catalogs: Vec<_> = SOURCES.iter().filter(|s| s.is_catalog()).collect();
    ui::stage(format!("downloading {} catalogs", catalogs.len()));
    let progress = ui::bar(catalogs.len() as u64, "catalogs");
    let results: Vec<(&str, Result<usize, SourceError>)> =
        std::thread::scope(|scope| {
            let workers: Vec<_> = catalogs
                .iter()
                .map(|source| {
                    let (ctx, progress) = (&ctx, progress.clone());
                    (
                        source.id,
                        scope.spawn(move || {
                            let result = source.warm(ctx).unwrap_or(Ok(0));
                            progress.inc(1);
                            result
                        }),
                    )
                })
                .collect();
            workers
                .into_iter()
                .map(|(id, worker)| {
                    let result = worker.join().unwrap_or_else(|_| {
                        Err(SourceError::shape("the adapter crashed"))
                    });
                    (id, result)
                })
                .collect()
        });
    progress.finish_and_clear();
    services.cache.trim();

    let mut w = out.stdout();
    if out.json {
        let report: Vec<_> = results
            .iter()
            .map(|(id, r)| match r {
                Ok(n) => serde_json::json!({"id": id, "entries": n}),
                Err(e) => {
                    serde_json::json!({"id": id, "error": e.to_string()})
                }
            })
            .collect();
        writeln!(w, "{}", serde_json::to_string(&report)?)?;
        return Ok(());
    }
    for (id, result) in &results {
        match result {
            Ok(n) => writeln!(w, "{id:<22} {n} entries")?,
            Err(e) => writeln!(w, "{id:<22} failed: {e}")?,
        }
    }
    Ok(())
}

fn info(cache: &Cache, out: &Out) -> Result<()> {
    let usage = cache.usage();
    let mut w = out.stdout();
    if out.json {
        let report = serde_json::json!({
            "path": cache.root(),
            "files": usage.files,
            "bytes": usage.bytes,
            "budget_files": BUDGET_FILES,
            "budget_bytes": BUDGET_BYTES,
        });
        writeln!(w, "{}", serde_json::to_string(&report)?)?;
        return Ok(());
    }
    writeln!(w, "{}", cache.root().display())?;
    writeln!(
        w,
        "{} files, {} of {} budget ({} files max)",
        usage.files,
        human_bytes(usage.bytes),
        human_bytes(BUDGET_BYTES),
        BUDGET_FILES
    )?;
    Ok(())
}
