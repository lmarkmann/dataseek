use std::io::{IsTerminal, Stderr, Stdout, Write};

use anstream::AutoStream;
use anstream::stream::RawStream;
use serde::Serialize;

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
    pub jq: Option<String>,
    pub plain: bool,
    pub progress: bool,
    color: ColorChoice,
}

impl Out {
    pub fn resolve(cli: &Cli) -> Self {
        Self {
            verbosity: Verbosity::from_counts(cli.quiet, cli.verbose),
            json: cli.json || cli.jq.is_some(),
            jq: cli.jq.clone(),
            plain: cli.plain,
            progress: !cli.no_progress,
            color: resolve_color(cli.color, cli.no_color, cli.plain),
        }
    }

    /// stdout filtered per the resolved --color choice, so callers can style
    /// unconditionally.
    pub fn stdout(&self) -> AutoStream<Stdout> {
        stream(std::io::stdout(), self.color)
    }

    pub fn stderr(&self) -> AutoStream<Stderr> {
        stream(std::io::stderr(), self.color)
    }

    /// The one way a command prints `--json`: through `--jq` when given,
    /// indented for a person at a terminal, one line for a pipe.
    pub fn json(&self, value: &impl Serialize) -> anyhow::Result<()> {
        let mut w = self.stdout();
        if let Some(filter) = &self.jq {
            for line in crate::jq::run(filter, &serde_json::to_vec(value)?)? {
                writeln!(w, "{line}")?;
            }
        } else if std::io::stdout().is_terminal() {
            writeln!(w, "{}", serde_json::to_string_pretty(value)?)?;
        } else {
            writeln!(w, "{}", serde_json::to_string(value)?)?;
        }
        Ok(())
    }

    /// A diagnostic for `-v` and above, on stderr. Unlike [`crate::ui`] it
    /// still prints when stderr is redirected, which is the point of
    /// `-v 2> log`.
    pub fn note(&self, msg: &str) {
        if self.verbosity >= Verbosity::Verbose {
            let _ = writeln!(self.stderr(), "{msg}");
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

    /// Asked separately from [`Self::color_on_stdout`]: a run can pipe its data
    /// and still be watched by a human.
    pub fn color_on_stderr(&self) -> bool {
        self.color_for(&std::io::stderr())
    }

    /// `url` as an OSC 8 hyperlink on a color terminal, plain text anywhere
    /// else, so a pipe never carries the escape.
    pub fn link(&self, url: &str) -> String {
        if std::io::stdout().is_terminal() && self.color_on_stdout() {
            format!("\u{1b}]8;;{url}\u{1b}\\{url}\u{1b}]8;;\u{1b}\\")
        } else {
            url.to_owned()
        }
    }
}

/// `--plain` and `--no-color` are shorthands for `--color=never`. Auto is
/// left to AutoStream, which reads the terminal and NO_COLOR.
pub fn resolve_color(
    color: ColorChoice,
    no_color: bool,
    plain: bool,
) -> ColorChoice {
    if plain || no_color { ColorChoice::Never } else { color }
}

/// Columns a human line may use, or `None` when stdout is not a terminal and
/// lines should stay whole. `COLUMNS` wins over the measured width.
pub fn width() -> Option<usize> {
    if !std::io::stdout().is_terminal() {
        return None;
    }
    std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).or_else(|| {
        terminal_size::terminal_size().map(|(w, _)| usize::from(w.0))
    })
}

fn stream<S: RawStream>(raw: S, color: ColorChoice) -> AutoStream<S> {
    match color {
        ColorChoice::Always => AutoStream::always(raw),
        ColorChoice::Never => AutoStream::never(raw),
        ColorChoice::Auto => AutoStream::auto(raw),
    }
}

/// `text` cut to `width` columns with an ellipsis, counting chars. Wide
/// glyphs make a line slightly long, never wrong.
pub fn fit(text: &str, width: Option<usize>) -> String {
    match width {
        Some(w) if text.chars().count() > w => {
            let kept: String =
                text.chars().take(w.saturating_sub(1)).collect();
            format!("{}\u{2026}", kept.trim_end())
        }
        _ => text.to_owned(),
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
            jq: None,
            color: ColorChoice::Auto,
            no_color: false,
            plain: false,
            no_progress: false,
            cache_dir: None,
            connect_timeout: 10,
            command: Some(Command::Doctor),
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
    fn jq_implies_json() {
        assert!(out(|c| c.jq = Some(".".to_owned())).json);
    }

    #[test]
    fn fit_cuts_long_lines_and_marks_the_cut() {
        assert_eq!(fit("short", Some(10)), "short");
        assert_eq!(fit("a long title here", Some(8)), "a long\u{2026}");
        assert_eq!(fit("a long title here", None), "a long title here");
    }
}
