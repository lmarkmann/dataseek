//! `doctor`: report readiness at a glance. The support command every good CLI
//! grows (yoink, brew, gh). It is also the template's worked example of the
//! whole UI stack: stderr narration and a progress sweep through
//! [`crate::ui`], stdout summary styled through [`crate::palette`],
//! directories from [`crate::paths`]. Replace these checks with ones your tool
//! actually needs (a reachable server, a found dependency, a valid token).

use std::io::Write;

use anyhow::Result;
use clap::builder::styling::Style;
use serde::Serialize;

use crate::output::Out;
use crate::{palette, paths, ui};

#[derive(Serialize)]
struct Check {
    label: &'static str,
    detail: String,
    ok: bool,
}

pub fn run(out: &Out) -> Result<()> {
    ui::stage("inspecting environment");
    let checks = gather(out)?;

    // Narrate the sweep on stderr; the bar and lines vanish in a pipe.
    let progress = ui::bar(checks.len() as u64, "checks");
    for check in &checks {
        if check.ok {
            ui::ok(format!("{}: {}", check.label, check.detail));
        } else {
            ui::warn(format!("{}: {}", check.label, check.detail));
        }
        progress.inc(1);
    }
    progress.finish_and_clear();

    report(out, &checks)
}

/// The checks themselves. This is the part you replace: swap these for a
/// reachable host, a binary on `PATH`, a readable config file.
fn gather(out: &Out) -> Result<Vec<Check>> {
    let p = paths::resolve()?;

    let exists = |path: &std::path::Path| {
        if path.exists() {
            "exists".to_owned()
        } else {
            "not yet created".to_owned()
        }
    };

    let checks = vec![
        Check {
            label: "version",
            detail: env!("CARGO_PKG_VERSION").to_owned(),
            ok: true,
        },
        Check {
            label: "config",
            detail: format!("{} ({})", p.config.display(), exists(&p.config)),
            ok: true,
        },
        Check {
            label: "cache",
            detail: format!("{} ({})", p.cache.display(), exists(&p.cache)),
            ok: true,
        },
        Check {
            label: "state",
            detail: format!("{} ({})", p.state.display(), exists(&p.state)),
            ok: true,
        },
        Check {
            label: "color",
            // Informational, not a verdict: color off in a pipe is correct,
            // not a problem. Real checks you add (a reachable host, a found
            // binary) set ok:false to drive the warn path and the closing
            // summary. Deliberately not enumerating the reasons: anstream
            // weighs a terminal check, NO_COLOR, CLICOLOR, CLICOLOR_FORCE,
            // TERM and CI, and a list here would go stale the next time it
            // learns another.
            detail: if out.color_on_stdout() {
                "enabled".to_owned()
            } else {
                "disabled (piped, or suppressed by the environment)".to_owned()
            },
            ok: true,
        },
        Check {
            label: "unicode",
            detail: if out.plain {
                "ascii (--plain)".to_owned()
            } else {
                "enabled".to_owned()
            },
            ok: true,
        },
    ];

    Ok(checks)
}

/// The readiness summary on stdout: JSON when asked for, a styled table
/// otherwise.
fn report(out: &Out, checks: &[Check]) -> Result<()> {
    let all_ok = checks.iter().all(|c| c.ok);

    // The summary is data, so the global --json flag applies here too: a
    // script running `doctor --json` gets a parseable report, not a styled
    // table.
    if out.json {
        let report = serde_json::json!({
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "ready": all_ok,
            "checks": checks,
        });
        writeln!(out.stdout(), "{}", serde_json::to_string(&report)?)?;
        return Ok(());
    }

    let mut w = out.stdout();
    let (good, bad) = if out.plain { ("+", "!") } else { ("✓", "⚠") };
    let title = palette::accent();
    writeln!(
        w,
        "{title}{} {}{title:#}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )?;
    writeln!(w)?;
    let dim = palette::muted();
    for check in checks {
        let (mark, style): (&str, Style) = if check.ok {
            (good, palette::success())
        } else {
            (bad, palette::warning())
        };
        writeln!(
            w,
            "  {style}{mark}{style:#} {:<8} {dim}{}{dim:#}",
            check.label, check.detail
        )?;
    }
    writeln!(w)?;

    let summary = palette::success();
    if all_ok {
        writeln!(w, "{summary}Ready.{summary:#}")?;
    } else {
        let warn = palette::warning();
        writeln!(w, "{warn}Ready, with notes above.{warn:#}")?;
    }
    Ok(())
}
