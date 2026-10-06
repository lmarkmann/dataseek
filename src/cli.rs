use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::palette;

/// When to colorize output. The de-facto standard flag (git, ripgrep, fd).
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ColorChoice {
    /// Color when stdout is a terminal and NO_COLOR is unset.
    #[default]
    Auto,
    /// Always color, even through a pipe (for `| less -R`).
    Always,
    /// Never color.
    Never,
}

#[derive(Parser)]
#[command(
    name = "dataseek",
    version,
    about = env!("CARGO_PKG_DESCRIPTION"),
    after_help = "Examples:\n  \
        dataseek doctor\n  \
        dataseek completion fish > ~/.config/fish/completions/dataseek.fish\n  \
        dataseek man | man -l -\n\n\
        dsk is the same program under a shorter name: dsk doctor",
    arg_required_else_help = true,
    disable_help_subcommand = true,
    styles = palette::help()
)]
pub struct Cli {
    /// Errors and final result only.
    #[arg(short = 'q', long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Explain what is happening on stderr.
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Emit JSON to stdout instead of text.
    #[arg(long, global = true)]
    pub json: bool,

    /// When to colorize output: auto, always, or never.
    #[arg(long, value_name = "WHEN", default_value = "auto", global = true)]
    pub color: ColorChoice,

    /// Disable ANSI colors; shorthand for --color=never (or set NO_COLOR).
    #[arg(long, global = true)]
    pub no_color: bool,

    /// ASCII-only output; implies --color=never.
    #[arg(long, global = true)]
    pub plain: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Check the environment and report readiness.
    Doctor,

    /// Print a shell completion script to stdout.
    Completion {
        /// Target shell (fish, bash, zsh, ...).
        shell: Shell,
    },

    /// Print the man page, in roff, to stdout.
    #[command(after_help = "Examples:\n  \
        dataseek man | man -l -\n  \
        dataseek man > ~/.local/share/man/man1/dataseek.1")]
    Man,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    // Catches malformed definitions (duplicate short flags, bad defaults) that
    // are not compile errors.
    #[test]
    fn the_definition_is_well_formed() {
        Cli::command().debug_assert();
    }
}
