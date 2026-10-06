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
        // --plain and --no-color are shorthands for --color=never. Auto is
        // left to AutoStream, which reads the terminal and NO_COLOR.
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

    /// stdout filtered per the resolved --color choice, so callers can style
    /// unconditionally.
    pub fn stdout(&self) -> AutoStream<Stdout> {
        let out = std::io::stdout();
        match self.color {
            ColorChoice::Always => AutoStream::always(out),
            ColorChoice::Never => AutoStream::never(out),
            ColorChoice::Auto => AutoStream::auto(out),
        }
    }

    /// A diagnostic for `-v` and above, on stderr. Unlike [`crate::ui`] it
    /// still prints when stderr is redirected, which is the point of
    /// `-v 2> log`.
    pub fn note(&self, msg: &str) {
        if self.verbosity >= Verbosity::Verbose {
            let _ = writeln!(anstream::stderr(), "{msg}");
        }
    }

    /// Would stdout carry color right now? For reporting only; the stream
    /// strips escapes on its own.
    pub fn color_on_stdout(&self) -> bool {
        self.color_for(&std::io::stdout())
    }

    /// Asks anstream, the same authority the stream uses. A hand-rolled
    /// `is_terminal() && NO_COLOR` misses CLICOLOR_FORCE, CLICOLOR, TERM and CI.
    fn color_for(&self, stream: &impl RawStream) -> bool {
        match self.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                AutoStream::choice(stream) != anstream::ColorChoice::Never
            }
        }
    }

    /// Should the stderr [`crate::ui`] layer draw? Only when stderr is a
    /// terminal and the run is neither `--json` nor `--quiet`.
    pub fn stderr_is_rich(&self) -> bool {
        std::io::stderr().is_terminal()
            && !self.json
            && self.verbosity > Verbosity::Quiet
    }

    /// Asked separately from [`Self::color_on_stdout`]: a run can pipe its data
    /// and still be watched by a human.
    pub fn color_on_stderr(&self) -> bool {
        self.color_for(&std::io::stderr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;

    // Flag resolution is the pure logic; the rest of output needs a real
    // process, which tests/cli.rs covers.
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
        // -vv is not a third level.
        assert_eq!(out(|c| c.verbose = 3).verbosity, Verbosity::Verbose);
    }

    #[test]
    fn plain_and_no_color_are_shorthands_for_never() {
        // Always, because Auto is already false off a terminal.
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
        // stderr is never a terminal here; these pin the two flags that must
        // switch the layer off even on one.
        assert!(!out(|c| c.json = true).stderr_is_rich());
        assert!(!out(|c| c.quiet = true).stderr_is_rich());
    }
}
