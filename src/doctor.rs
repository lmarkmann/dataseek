//! `doctor`: report readiness at a glance, as a styled summary on stdout. The
//! checks take milliseconds, so nothing is narrated while they run.

use std::io::Write;

use anyhow::Result;
use clap::builder::styling::Style;
use serde::Serialize;

use crate::cache::{self, Cache};
use crate::credentials::{Credentials, Key};
use crate::find::human_bytes;
use crate::output::{self, Out};
use crate::sources::SOURCES;
use crate::{palette, paths};

#[derive(Serialize)]
struct Check {
    label: String,
    detail: String,
    ok: bool,
}

pub fn run(out: &Out) -> Result<()> {
    let checks = gather(out)?;
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

    let mut checks = vec![
        Check {
            label: "version".to_owned(),
            detail: env!("CARGO_PKG_VERSION").to_owned(),
            ok: true,
        },
        Check {
            label: "config".to_owned(),
            detail: format!(
                "{} ({})",
                dirs.config.display(),
                existence(&dirs.config)
            ),
            ok: true,
        },
        Check {
            label: "cache".to_owned(),
            detail: format!(
                "{} ({})",
                dirs.cache.display(),
                existence(&dirs.cache)
            ),
            ok: true,
        },
        Check {
            label: "state".to_owned(),
            detail: format!(
                "{} ({})",
                dirs.state.display(),
                existence(&dirs.state)
            ),
            ok: true,
        },
        Check {
            label: "color".to_owned(),
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
            label: "unicode".to_owned(),
            detail: if out.plain {
                "ascii (--plain)".to_owned()
            } else {
                "enabled".to_owned()
            },
            ok: true,
        },
    ];
    checks.extend(keys(&dirs.config));
    checks.extend(opt_ins());
    let usage = Cache::new(dirs.cache.clone()).usage();
    checks.push(Check {
        label: "cache use".to_owned(),
        detail: format!(
            "{} files, {} of {}",
            usage.files,
            human_bytes(usage.bytes),
            human_bytes(cache::budget_bytes())
        ),
        ok: usage.bytes <= cache::budget_bytes(),
    });

    Ok(checks)
}

/// One line per API key: where it was found, or where to get one. A missing
/// key is never a failure, since sources without one run anonymously or are
/// skipped; a key file other users can read is.
fn keys(config: &std::path::Path) -> Vec<Check> {
    let creds = Credentials::load(config);
    let mut checks: Vec<Check> = Key::ALL
        .iter()
        .map(|&key| Check {
            label: format!("key {}", key.env_var()),
            detail: match creds.origin(key) {
                Some(origin) => format!("set ({origin})"),
                None => format!("not set, get one at {}", key.signup()),
            },
            ok: true,
        })
        .collect();
    for path in &creds.loose_files {
        checks.push(Check {
            label: "key file".to_owned(),
            detail: format!(
                "{} is readable by other users; run chmod 600 on it",
                path.display()
            ),
            ok: false,
        });
    }
    checks
}

/// One line per source that is asked only when named, with the reason.
fn opt_ins() -> impl Iterator<Item = Check> {
    SOURCES.iter().filter_map(|s| {
        s.opt_in.map(|reason| Check {
            label: format!("opt-in {}", s.id),
            detail: format!("asked only with -s {}: {reason}", s.id),
            ok: true,
        })
    })
}

/// The readiness summary on stdout: JSON when asked for, a styled table
/// otherwise.
fn report(out: &Out, checks: &[Check]) -> Result<()> {
    let all_ok = checks.iter().all(|c| c.ok);

    if out.json {
        return out.json(&serde_json::json!({
            "schema": "dataseek-doctor/1",
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "ready": all_ok,
            "checks": checks,
        }));
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
    // Two spaces, the mark, a space, the label column, a space.
    let indent = 29;
    let room = output::room(indent);
    for check in checks {
        let (mark, style): (&str, Style) = if check.ok {
            (good, palette::success())
        } else {
            (bad, palette::warning())
        };
        let mut detail = output::wrap(&check.detail, room).into_iter();
        writeln!(
            w,
            "  {style}{mark}{style:#} {:<24} {dim}{}{dim:#}",
            check.label,
            detail.next().unwrap_or_default()
        )?;
        for line in detail {
            writeln!(w, "{:indent$}{dim}{line}{dim:#}", "")?;
        }
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
