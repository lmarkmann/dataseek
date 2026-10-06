use clap::builder::PossibleValuesParser;
use clap::{Args, Parser, Subcommand, ValueEnum};
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
        dataseek search sea surface temperature\n  \
        dataseek search imagenet -s huggingface,kaggle --json | jq '.results[0]'\n  \
        dataseek sources\n  \
        dataseek bench \"air quality\" \"gene expression\"\n  \
        dataseek inspect https://zenodo.org/records/1234567\n\n\
        dsk is the same program under a shorter name: dsk search mnist",
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

/// Which sources to ask, shared by `search` and `bench`.
#[derive(Args, Clone, Debug, Default)]
pub struct Selection {
    /// Only these sources, comma-separated (see `dataseek sources`).
    #[arg(
        short = 's',
        long = "source",
        value_name = "IDS",
        value_delimiter = ',',
        value_parser = source_ids(),
        hide_possible_values = true
    )]
    pub only: Vec<String>,

    /// Leave these sources out, comma-separated.
    #[arg(
        short = 'x',
        long,
        value_name = "IDS",
        value_delimiter = ',',
        value_parser = source_ids(),
        hide_possible_values = true
    )]
    pub exclude: Vec<String>,

    /// Only sources in these categories, comma-separated.
    #[arg(
        short = 'c',
        long = "category",
        value_name = "NAMES",
        value_delimiter = ','
    )]
    pub categories: Vec<crate::sources::Category>,

    /// How many results to ask each source for (1-100).
    #[arg(
        long,
        value_name = "N",
        default_value_t = 10,
        value_parser = clap::value_parser!(u16).range(1..=100)
    )]
    pub per_source: u16,
}

fn source_ids() -> PossibleValuesParser {
    PossibleValuesParser::new(crate::sources::SOURCES.iter().map(|s| s.id))
}

#[derive(Subcommand)]
pub enum Command {
    /// Search every source at once, merged and deduplicated.
    #[command(
        visible_alias = "s",
        after_help = "Examples:\n  \
        dataseek search sea surface temperature\n  \
        dataseek search mnist -s huggingface,kaggle,openml\n  \
        dataseek search unemployment -x google -n 50\n  \
        dataseek search inflation -c economics,finance\n  \
        dataseek search census --json | jq -r '.results[].url'"
    )]
    Search {
        /// What to look for; several words form one query.
        #[arg(required = true, value_name = "QUERY")]
        query: Vec<String>,

        #[command(flatten)]
        selection: Selection,

        /// How many merged results to print.
        #[arg(short = 'n', long, value_name = "N", default_value_t = 20)]
        limit: usize,

        /// Ask every source again instead of reusing cached answers.
        #[arg(long)]
        refresh: bool,

        /// Seconds to wait for slow sources before printing; 0 waits for all.
        #[arg(long, value_name = "SECS", default_value_t = 20)]
        timeout: u64,
    },

    /// List every source: what it covers, its protocol, its key, its docs.
    #[command(after_help = "Examples:\n  \
        dataseek sources\n  \
        dataseek sources --json | jq -r '.[] | select(.key == \"missing\") | .id'")]
    Sources,

    /// Time every source on real queries and measure their overlap.
    #[command(after_help = "Examples:\n  \
        dataseek bench\n  \
        dataseek bench \"air quality\" \"protein structure\" --json > bench.json\n  \
        dataseek bench mnist -s huggingface,kaggle,openml,uci\n\
        \n\
        Bench bypasses the cache and asks every chosen source once per query,\n\
        so it sends queries x sources requests. Without queries it uses a\n\
        fixed set chosen to touch every category.")]
    Bench {
        /// Queries to time; each is one argument (quote multi-word queries).
        #[arg(value_name = "QUERY")]
        queries: Vec<String>,

        #[command(flatten)]
        selection: Selection,
    },

    /// Read a dataset page's schema.org Dataset or Croissant metadata.
    #[command(after_help = "Examples:\n  \
        dataseek inspect https://zenodo.org/records/1234567\n  \
        dataseek inspect https://huggingface.co/datasets/stanfordnlp/imdb --json")]
    Inspect {
        /// The dataset's landing page.
        #[arg(value_name = "URL")]
        url: String,
    },

    /// Show, warm or clear the cache.
    #[command(subcommand)]
    Cache(CacheAction),

    /// Check the environment, keys and cache, and report readiness.
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

#[derive(Subcommand, Clone, Copy)]
pub enum CacheAction {
    /// Where the cache lives, how full it is, and its budget.
    Info,
    /// Download every catalog-searched source's list now.
    Warm,
    /// Delete every cached result and catalog.
    Clear,
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
