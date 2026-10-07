//! dataseek: search for datasets from the terminal.
//!
//! The crate is a library only so that one program can ship under two names:
//! `src/main.rs` builds `dataseek` and `src/bin/dsk.rs` builds `dsk`, and both
//! call [`main`]. Nothing here is a public API; every module is private, and
//! the `internals` feature exposes only what `benches/` measures.
//! This file holds the entry point: SIGPIPE handling, argument parsing,
//! dispatch, and the error printer.

#![cfg_attr(
    feature = "internals",
    allow(
        missing_docs,
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        clippy::missing_errors_doc,
        reason = "public-API lints; internals makes private items reachable for benches only"
    )
)]

mod bench;
mod cache;
mod cache_cmd;
mod catalog;
mod cli;
mod credentials;
mod dedup;
mod doctor;
mod find;
mod fs;
mod help;
mod http;
mod inspect;
mod listing;
mod output;
mod palette;
mod paths;
mod record;
mod search;
mod sources;
mod ui;

/// What `benches/search.rs` measures. Not an API: it follows the code it
/// points at.
#[cfg(feature = "internals")]
#[doc(hidden)]
pub mod internals {
    pub use crate::cache::{Cache, Kind};
    pub use crate::catalog::search as catalog_search;
    pub use crate::dedup::{merge, weigh};
    pub use crate::record::{Dataset, clean};
    pub use crate::sources::eurostat::parse as eurostat_toc;
    pub use crate::sources::sdmx::flows as sdmx_dataflows;
}

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use clap::error::ErrorKind::{
    DisplayHelp, DisplayHelpOnMissingArgumentOrSubcommand, DisplayVersion,
};
use clap::{CommandFactory, FromArgMatches, ValueEnum};

use cli::{Cli, ColorChoice, Command};
use output::Out;

/// Rust ignores SIGPIPE, so `dataseek search x | head` would panic on the next
/// write. Restore the default: die quietly with 141. Unix only; Windows reports
/// a closed pipe as a write error, which [`report`] handles.
#[cfg(unix)]
fn restore_sigpipe() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let _ = signal_hook::flag::register_conditional_default(
        signal_hook::consts::SIGPIPE,
        Arc::new(AtomicBool::new(true)),
    );
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

/// Run the command line named by the process arguments. Shared by the
/// `dataseek` and `dsk` binaries; clap reads the invoked name from `argv[0]`,
/// so usage lines and completions follow whichever name was typed.
#[must_use]
pub fn main() -> ExitCode {
    restore_sigpipe();

    let args: Vec<OsString> = std::env::args_os().collect();
    let early = Early::scan(&args);
    let parsed = Cli::command()
        .color(early.color)
        .try_get_matches_from(&args)
        .and_then(|matches| Cli::from_arg_matches(&matches));
    let cli = match parsed {
        Ok(cli) => cli,
        Err(err) => return usage(&err, &early),
    };

    let out = Out::resolve(&cli);
    ui::init(&out);
    paths::relocate_cache(cli.cache_dir.clone());
    http::connect_within(Duration::from_secs(cli.connect_timeout));

    match run(cli, &out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report(&err, &out),
    }
}

/// What has to be known before clap parses, because clap's own help and
/// errors are printed before [`Out`] exists: the color choice and whether the
/// caller wants JSON.
struct Early {
    color: clap::ColorChoice,
    json: bool,
}

impl Early {
    fn scan(args: &[OsString]) -> Self {
        let (mut color, mut no_color, mut plain, mut json) =
            (ColorChoice::Auto, false, false, false);
        let mut words = args.iter().skip(1).map(|a| a.to_string_lossy());
        while let Some(word) = words.next() {
            match word.as_ref() {
                "--" => break,
                "--no-color" => no_color = true,
                "--plain" => plain = true,
                "--json" => json = true,
                "--color" => {
                    if let Some(value) = words.next() {
                        color = ColorChoice::from_str(&value, true)
                            .unwrap_or(color);
                    }
                }
                other => {
                    if let Some(value) = other.strip_prefix("--color=") {
                        color = ColorChoice::from_str(value, true)
                            .unwrap_or(color);
                    }
                }
            }
        }
        let color = match output::resolve_color(color, no_color, plain) {
            ColorChoice::Auto => clap::ColorChoice::Auto,
            ColorChoice::Always => clap::ColorChoice::Always,
            ColorChoice::Never => clap::ColorChoice::Never,
        };
        Self { color, json }
    }
}

/// Help, version and usage errors from clap. Under `--json` a usage error is
/// an error event like any other, so an agent parses one shape.
fn usage(err: &clap::Error, early: &Early) -> ExitCode {
    let code = match err.kind() {
        DisplayHelp | DisplayVersion => return print_clap(err),
        // `dataseek cache` without an action: asking for help, not misusing.
        DisplayHelpOnMissingArgumentOrSubcommand => {
            let _ = print_clap(err);
            return ExitCode::SUCCESS;
        }
        _ => ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2)),
    };
    if !early.json {
        let _ = err.print();
        return code;
    }
    let text = err.render().to_string();
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let message = lines.next().unwrap_or_default();
    let hint = lines
        .find_map(|l| l.strip_prefix("tip: "))
        .unwrap_or("run with --help to see what is accepted");
    ui::event_line(&serde_json::json!({
        "schema": ui::EVENTS_SCHEMA,
        "event": "error",
        "message": message.strip_prefix("error: ").unwrap_or(message),
        "causes": [],
        "try": hint,
    }));
    code
}

fn print_clap(err: &clap::Error) -> ExitCode {
    let _ = err.print();
    ExitCode::SUCCESS
}

fn run(cli: Cli, out: &Out) -> anyhow::Result<()> {
    let Some(command) = cli.command else {
        return if out.json {
            help::run(None, out)
        } else {
            help::overview(out)
        };
    };
    match command {
        Command::Search {
            query,
            selection,
            limit,
            sort,
            refresh,
            offline,
            timeout,
        } => {
            if offline {
                http::go_offline();
            }
            let request = find::Request {
                words: &query,
                selection: &selection,
                limit,
                sort,
                refresh,
                offline,
                timeout: (timeout > 0).then_some(timeout),
            };
            find::run_search(&request, out)?;
        }
        Command::Sources => listing::run(out)?,
        Command::Bench { queries, selection } => {
            bench::run(&queries, &selection, out)?;
        }
        Command::Inspect { url, offline } => {
            if offline {
                http::go_offline();
            }
            inspect::run(&url, out)?;
        }
        Command::Cache(action) => cache_cmd::run(action, out)?,
        Command::Doctor => doctor::run(out)?,
        // clap_complete::generate panics on a failed write. Generating into a
        // Vec cannot fail, so the real write goes through the error path.
        Command::Completion { shell } => {
            let mut cmd = Cli::command();
            let mut script = Vec::new();
            clap_complete::generate(
                shell,
                &mut cmd,
                invoked_name(),
                &mut script,
            );
            out.stdout().write_all(&script)?;
        }
        // Rendered on demand so the page cannot drift from the flags.
        Command::Man => {
            let mut page = Vec::new();
            clap_mangen::Man::new(Cli::command()).render(&mut page)?;
            out.stdout().write_all(&page)?;
        }
        Command::Help { topic } => help::run(topic.as_deref(), out)?,
    }
    Ok(())
}

/// `dsk` when run as `dsk`, `dataseek` otherwise (a renamed or wrapped
/// binary still gets completions for the canonical name).
fn invoked_name() -> &'static str {
    let called =
        std::env::args_os().next().map(std::path::PathBuf::from).and_then(
            |path| path.file_stem().map(|s| s.to_string_lossy().into_owned()),
        );
    if called.as_deref() == Some("dsk") {
        "dsk"
    } else {
        env!("CARGO_PKG_NAME")
    }
}

/// Print the error on stderr as Error, Cause and Try lines, leaving stdout
/// clean; under `--json`, as one error event. Typed errors carry their
/// recovery hint in the message after a `Try:` marker; an error without one
/// was not anticipated, so the hint becomes the bug-report address.
fn report(err: &anyhow::Error, out: &Out) -> ExitCode {
    // A typed error with an io::Error as #[source] hides it from a downcast of
    // the outermost error, so walk the chain.
    let broken_pipe = err
        .chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|io_err| io_err.kind() == io::ErrorKind::BrokenPipe);
    if broken_pipe {
        return ExitCode::SUCCESS;
    }
    let message = err.to_string();
    let (what, hint) = match message.split_once("\n  Try:") {
        Some((what, hint)) => (what.to_owned(), hint.trim().to_owned()),
        None => (
            message.clone(),
            format!(
                "if this keeps happening, report it at {}/issues",
                env!("CARGO_PKG_REPOSITORY")
            ),
        ),
    };
    let causes: Vec<String> =
        err.chain().skip(1).map(ToString::to_string).collect();
    if out.json {
        ui::event_line(&serde_json::json!({
            "schema": ui::EVENTS_SCHEMA,
            "event": "error",
            "message": what,
            "causes": causes,
            "try": hint,
        }));
        return ExitCode::FAILURE;
    }
    let mut stderr = out.stderr();
    let red = palette::danger();
    let _ = writeln!(stderr, "{red}Error:{red:#} {what}");
    for cause in &causes {
        let _ = writeln!(stderr, "  Cause: {cause}");
    }
    let _ = writeln!(stderr, "  Try:   {hint}");
    ExitCode::FAILURE
}
