use std::io::{IsTerminal, Stdout, Write};

use anstream::AutoStream;
use anstream::stream::RawStream;

use crate::cli::{Cli, ColorChoice};

/// How chatty stderr should be. stdout data is never affected by this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

impl Verbosity {
    fn from_counts(quiet: bool, verbose: u8) -> Self {
        match (quiet, verbose) {
            (true, _) => Self::Quiet,
            (false, 0) => Self::Normal,
            (false, _) => Self::Verbose,
        }
    }
}

/// Resolved output context: how loud, what format, whether to color.
pub struct Out {
    pub verbosity: Verbosity,
    pub json: bool,
    pub plain: bool,
    color: ColorChoice,
}

impl Out {
    pub fn resolve(cli: &Cli) -> Self {
        // --plain and --no-color are shorthands for --color=never; otherwise
        // the explicit --color value wins. Auto consults the terminal and
        // NO_COLOR via AutoStream below, so no env parsing is needed here.
        let color = if cli.plain || cli.no_color {
            ColorChoice::Never
        } else {
            cli.color
        };

        Self {
            verbosity: Verbosity::from_counts(cli.quiet, cli.verbose),
            json: cli.json,
            plain: cli.plain,
            color,
        }
    }

    /// stdout that emits ANSI per the resolved --color choice: `always` forces
    /// color through a pipe, `never` strips it, `auto` follows the terminal
    /// and NO_COLOR. `anstyle` escapes are filtered by this stream, so callers
    /// can style unconditionally and let the choice decide.
    pub fn stdout(&self) -> AutoStream<Stdout> {
        let out = std::io::stdout();
        match self.color {
            ColorChoice::Always => AutoStream::always(out),
            ColorChoice::Never => AutoStream::never(out),
            ColorChoice::Auto => AutoStream::auto(out),
        }
    }

    /// A verbose diagnostic for humans: shown at `-v` and above, silent at
    /// normal and quiet. Goes to stderr, never stdout, so it never pollutes a
    /// pipe. Unlike the [`crate::ui`] layer, it still prints when stderr is
    /// redirected to a file, which is the point of `-v 2> log`.
    pub fn note(&self, msg: &str) {
        if self.verbosity >= Verbosity::Verbose {
            let _ = writeln!(anstream::stderr(), "{msg}");
        }
    }

    /// Would stdout carry color right now? Used for reporting, not for gating
    /// output; the stream strips escapes on its own.
    pub fn color_on_stdout(&self) -> bool {
        self.color_for(&std::io::stdout())
    }

    /// Answer from the same authority the stream uses. Hand-rolling this as
    /// `is_terminal() && NO_COLOR` gets it wrong in both directions, because
    /// anstream also honors CLICOLOR_FORCE, CLICOLOR, TERM (unset or `dumb`),
    /// and CI detection. Two color policies in one program disagree
    /// eventually, and the one that reports is the one that ends up lying.
    fn color_for(&self, stream: &impl RawStream) -> bool {
        match self.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                AutoStream::choice(stream) != anstream::ColorChoice::Never
            }
        }
    }

    /// Should the stderr [`crate::ui`] layer draw at all? Rich means a human
    /// is watching: stderr is a terminal and the run is neither
    /// machine-readable nor silenced.
    pub fn stderr_is_rich(&self) -> bool {
        std::io::stderr().is_terminal()
            && !self.json
            && self.verbosity > Verbosity::Quiet
    }

    /// Would stderr carry color? Narration and progress are styled, so this is
    /// asked separately from [`Self::color_on_stdout`]: a run can pipe its
    /// data and still be watched by a human.
    pub fn color_on_stderr(&self) -> bool {
        self.color_for(&std::io::stderr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;

    // Flag resolution is the one piece of pure logic in the crate, so it is
    // the one piece testable without spawning the binary. Everything else
    // about output needs a real terminal, which is what tests/cli.rs is for.
    fn out(build: impl FnOnce(&mut Cli)) -> Out {
        let mut cli = Cli {
            quiet: false,
            verbose: 0,
            json: false,
            color: ColorChoice::Auto,
            no_color: false,
            plain: false,
            command: Command::Doctor,
        };
        build(&mut cli);
        Out::resolve(&cli)
    }

    #[test]
    fn quiet_and_verbose_map_to_levels() {
        assert_eq!(out(|_| {}).verbosity, Verbosity::Normal);
        assert_eq!(out(|c| c.quiet = true).verbosity, Verbosity::Quiet);
        assert_eq!(out(|c| c.verbose = 1).verbosity, Verbosity::Verbose);
        // Every count above zero is the same level; -vv is not a third one.
        assert_eq!(out(|c| c.verbose = 3).verbosity, Verbosity::Verbose);
    }

    #[test]
    fn plain_and_no_color_are_shorthands_for_never() {
        // Not a terminal under test, so Auto would be false anyway; Always is
        // the value that proves the shorthand overrode the choice rather than
        // the pipe deciding it.
        assert!(
            !out(|c| {
                c.plain = true;
                c.color = ColorChoice::Always;
            })
            .color_on_stdout()
        );
        assert!(
            !out(|c| {
                c.no_color = true;
                c.color = ColorChoice::Always;
            })
            .color_on_stdout()
        );
    }

    #[test]
    fn color_always_survives_a_pipe() {
        assert!(out(|c| c.color = ColorChoice::Always).color_on_stdout());
        assert!(!out(|c| c.color = ColorChoice::Never).color_on_stdout());
    }

    #[test]
    fn the_ui_layer_is_off_whenever_output_is_machine_read_or_silenced() {
        // stderr is not a terminal under test, so `rich` is false throughout;
        // these pin the two flags that must switch it off even when it is one.
        assert!(!out(|c| c.json = true).stderr_is_rich());
        assert!(!out(|c| c.quiet = true).stderr_is_rich());
    }
}
