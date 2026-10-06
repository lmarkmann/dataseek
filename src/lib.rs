//! dataseek: search for datasets from the terminal.
//!
//! The crate is a library only so that one program can ship under two names:
//! `src/main.rs` builds `dataseek` and `src/bin/dsk.rs` builds `dsk`, and both
//! call [`main`]. Nothing here is a public API; every module is private.
//! This file holds the entry point: SIGPIPE handling, argument parsing,
//! dispatch, and the error printer.

mod cli;
mod doctor;
mod fs;
mod output;
mod palette;
mod paths;
mod ui;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::error::ErrorKind::{
    DisplayHelp, DisplayHelpOnMissingArgumentOrSubcommand,
};
use clap::{CommandFactory, Parser};

use cli::{Cli, Command};
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

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            return match err.kind() {
                // Naked invocation is a success, not a usage error.
                DisplayHelp | DisplayHelpOnMissingArgumentOrSubcommand => {
                    let _ = err.print();
                    ExitCode::SUCCESS
                }
                _ => {
                    let _ = err.print();
                    ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2))
                }
            };
        }
    };

    let out = Out::resolve(&cli);
    ui::init(&out);

    match run(&cli, &out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report(&err),
    }
}

fn run(cli: &Cli, out: &Out) -> anyhow::Result<()> {
    match &cli.command {
        Command::Doctor => doctor::run(out)?,
        // clap_complete::generate panics on a failed write. Generating into a
        // Vec cannot fail, so the real write goes through the error path.
        Command::Completion { shell } => {
            let mut cmd = Cli::command();
            let mut script = Vec::new();
            clap_complete::generate(
                *shell,
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

/// Print the error as Error / Cause lines on stderr, leaving stdout clean.
/// Recovery hints ("Try: ...") live inside the typed error messages.
fn report(err: &anyhow::Error) -> ExitCode {
    // A typed error with an io::Error as #[source] hides it from a downcast of
    // the outermost error, so walk the chain.
    let broken_pipe = err
        .chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|io_err| io_err.kind() == io::ErrorKind::BrokenPipe);
    if broken_pipe {
        return ExitCode::SUCCESS;
    }
    let mut stderr = anstream::stderr();
    let _ = writeln!(stderr, "Error: {err}");
    for cause in err.chain().skip(1) {
        let _ = writeln!(stderr, "  Cause: {cause}");
    }
    ExitCode::FAILURE
}
