//! `doctor`: report readiness at a glance. Narrates the sweep on stderr through
//! [`crate::ui`] and prints the styled summary on stdout.

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

fn gather(out: &Out) -> Result<Vec<Check>> {
    let dirs = paths::resolve()?;

    let existence = |path: &std::path::Path| {
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
            detail: format!(
                "{} ({})",
                dirs.config.display(),
                existence(&dirs.config)
            ),
            ok: true,
        },
        Check {
            label: "cache",
            detail: format!(
                "{} ({})",
                dirs.cache.display(),
                existence(&dirs.cache)
            ),
            ok: true,
        },
        Check {
            label: "state",
            detail: format!(
                "{} ({})",
                dirs.state.display(),
                existence(&dirs.state)
            ),
            ok: true,
        },
        Check {
            label: "color",
            // Informational, not a verdict. The reasons stay unlisted because
            // anstream's rules (NO_COLOR, CLICOLOR, TERM, CI, ...) keep moving.
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
