//! Entry point: SIGPIPE handling, argument parsing, dispatch, and the error
//! printer. Every module below is a piece of the CLI quality contract; see
//! `README.md` for the statement of it.

mod cli;
mod count;
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

/// Rust ignores SIGPIPE by default, so `dataseek count | head` would panic
/// on the next write. Restore the Unix default: die quietly with 141.
///
/// Signals are a Unix concept, and `signal_hook::consts::SIGPIPE` does not
/// exist on Windows, so both the call and the dependency are gated. Windows
/// closes the read end of a pipe with an ordinary write error instead, which
/// the broken-pipe arm of [`report`] already handles.
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

fn main() -> ExitCode {
    restore_sigpipe();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            return match err.kind() {
                // Naked invocation asked for help: that is a success, not a
                // usage error.
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

    match run(cli, &out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report(&err),
    }
}

fn run(cli: Cli, out: &Out) -> anyhow::Result<()> {
    match cli.command {
        Command::Count { file } => count::run(file.as_deref(), out)?,
        Command::Doctor => doctor::run(out)?,
        // clap_complete::generate returns (), and every shell generator inside
        // it ends in `.expect("failed to write completion file")`. Writing
        // into a Vec cannot fail, so that expect becomes unreachable and the
        // real write goes through the normal error path instead of panicking
        // with 101.
        Command::Completion { shell } => {
            let mut cmd = Cli::command();
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut cmd, "dataseek", &mut script);
            out.stdout().write_all(&script)?;
        }
        // roff is data, so it goes to stdout like every other result.
        // Rendering on demand rather than in a build script keeps the page in
        // step with the flags automatically, and keeps the manifest free of a
        // build dependency the clone would have to reason about.
        Command::Man => {
            let mut page = Vec::new();
            clap_mangen::Man::new(Cli::command()).render(&mut page)?;
            out.stdout().write_all(&page)?;
        }
    }
    Ok(())
}

/// Print the error as Error / Cause lines on stderr, leaving stdout clean.
/// Recovery hints ("Try: ...") live inside the typed error messages.
fn report(err: &anyhow::Error) -> ExitCode {
    // Walk the chain rather than downcast the outermost error. anyhow's
    // context wrappers stay transparent to a downcast, but a typed error
    // carrying an io::Error as #[source] does not, and that is the shape
    // count.rs teaches.
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
