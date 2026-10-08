//! `cache info`, `cache warm` and `cache clear`.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::cache::{self, BUDGET_FILES, Cache};
use crate::cli::CacheAction;
use crate::find::human_bytes;
use crate::http::SourceError;
use crate::output::Out;
use crate::sources::{SOURCES, Services};
use crate::{paths, ui};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "{} of {} failed to download: {}\n  Try:   run `dataseek cache warm` again later; searches still use what did download",
        failed.len(),
        crate::ui::count(*total, "catalog"),
        failed.join(", ")
    )]
    Partial { failed: Vec<&'static str>, total: usize },
}

pub fn run(action: CacheAction, out: &Out) -> Result<()> {
    let cache = Cache::new(paths::resolve()?.cache);
    match action {
        CacheAction::Info => info(&cache, out),
        CacheAction::Warm { only, dry_run } => warm(out, &only, dry_run),
        CacheAction::Clear { dry_run } => clear(&cache, out, dry_run),
    }
}

/// The same report either way; `--dry-run` only skips the removal.
fn clear(cache: &Cache, out: &Out, dry_run: bool) -> Result<()> {
    let usage = cache.usage();
    if !dry_run {
        cache.clear().with_context(|| {
            format!(
                "cannot remove {}\n  Try:   check that you can write to it",
                cache.root().display()
            )
        })?;
    }
    if out.json {
        return out.json(&serde_json::json!({
            "schema": "dataseek-cache-clear/1",
            "path": cache.root(),
            "files": usage.files,
            "bytes": usage.bytes,
            "dry_run": dry_run,
        }));
    }
    ui::ok(format!(
        "{} {} ({} files, {})",
        if dry_run { "would clear" } else { "cleared" },
        cache.root().display(),
        usage.files,
        human_bytes(usage.bytes)
    ));
    Ok(())
}

/// Every catalog source, or the ones named, downloads in parallel, with no
/// deadline. Successes go to stdout, each failure to stderr, and any failure
/// fails the run.
fn warm(out: &Out, only: &[String], dry_run: bool) -> Result<()> {
    let catalogs: Vec<_> = SOURCES
        .iter()
        .filter(|s| s.is_catalog())
        .filter(|s| only.is_empty() || only.iter().any(|id| id == s.id))
        .collect();
    let results: Vec<(&str, Option<Result<usize, SourceError>>)> = if dry_run {
        catalogs.iter().map(|s| (s.id, None)).collect()
    } else {
        download(&catalogs)?
    };

    if out.json {
        out.json(&serde_json::json!({
            "schema": "dataseek-cache-warm/1",
            "dry_run": dry_run,
            "catalogs": results
                .iter()
                .map(|(id, r)| match r {
                    None => serde_json::json!({"id": id, "entries": null}),
                    Some(Ok(n)) => serde_json::json!({"id": id, "entries": n}),
                    Some(Err(e)) => {
                        serde_json::json!({"id": id, "error": e.to_string()})
                    }
                })
                .collect::<Vec<_>>(),
        }))?;
    } else {
        let mut w = out.stdout();
        for (id, result) in &results {
            match result {
                None => writeln!(w, "{id:<22} would download")?,
                Some(Ok(n)) => writeln!(w, "{id:<22} {n} entries")?,
                Some(Err(e)) => ui::warn(format!("{id}: {e}")),
            }
        }
    }

    let failed: Vec<_> = results
        .iter()
        .filter(|(_, r)| matches!(r, Some(Err(_))))
        .map(|(id, _)| *id)
        .collect();
    if !failed.is_empty() {
        return Err(Error::Partial { failed, total: results.len() }.into());
    }
    if !dry_run {
        ui::ok(format!("{} downloaded", ui::count(results.len(), "catalog")));
    }
    Ok(())
}

type Downloads<'a> = Vec<(&'a str, Option<Result<usize, SourceError>>)>;

fn download<'a>(
    catalogs: &[&'a crate::sources::Source],
) -> Result<Downloads<'a>> {
    let services = Services::load()?;
    // A cache that cannot be written would report every download as a
    // success and keep none of them.
    services.cache.check_writable().with_context(|| {
        format!(
            "cannot write the cache at {}\n  Try:   check its permissions, or pick another with --cache-dir",
            services.cache.root().display()
        )
    })?;
    let ctx = services.ctx(true);
    ui::stage(format!("downloading {}", ui::count(catalogs.len(), "catalog")));
    let progress = ui::bar(catalogs.len() as u64, "catalogs");
    let results = std::thread::scope(|scope| {
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
                (id, Some(result))
            })
            .collect()
    });
    progress.finish_and_clear();
    services.cache.trim();
    Ok(results)
}

/// Download these catalogs in a detached `cache warm` that outlives this
/// process, so a download a search stopped waiting for still reaches the
/// cache. A catalog another search set downloading within
/// [`cache::WARMING_TTL`] is left to that download. False when none could
/// start.
pub fn warm_in_background(cache: &Cache, ids: &[&str]) -> bool {
    let ids: Vec<&str> =
        ids.iter().copied().filter(|id| !cache.warming(id)).collect();
    if ids.is_empty() {
        return true;
    }
    let Ok(program) = std::env::current_exe() else { return false };
    let mut command = Command::new(program);
    command
        .args(["--quiet", "cache", "warm", "--source"])
        .arg(ids.join(","))
        .env("DATASEEK_CACHE_DIR", cache.root())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Out of the terminal's foreground group, so a Ctrl-C at the prompt
    // after the search does not reach it.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    if command.spawn().is_err() {
        return false;
    }
    for id in ids {
        cache.mark_warming(id);
    }
    true
}

fn info(cache: &Cache, out: &Out) -> Result<()> {
    let usage = cache.usage();
    if out.json {
        return out.json(&serde_json::json!({
            "schema": "dataseek-cache-info/1",
            "path": cache.root(),
            "files": usage.files,
            "bytes": usage.bytes,
            "budget_files": BUDGET_FILES,
            "budget_bytes": cache::budget_bytes(),
        }));
    }
    let mut w = out.stdout();
    writeln!(w, "{}", cache.root().display())?;
    writeln!(
        w,
        "{} files, {} of {} budget{} ({} files max)",
        usage.files,
        human_bytes(usage.bytes),
        human_bytes(cache::budget_bytes()),
        if cache::budget_from_env() {
            format!(", set by {}", cache::BUDGET_VAR)
        } else {
            String::new()
        },
        BUDGET_FILES
    )?;
    Ok(())
}
