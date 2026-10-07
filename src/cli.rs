use std::path::PathBuf;

use clap::builder::PossibleValuesParser;
use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::palette;

/// When to colorize output. The de-facto standard flag (git, ripgrep, fd).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// Color when stdout is a terminal and NO_COLOR is unset.
    #[default]
    Auto,
    /// Always color, even through a pipe (for `| less -R`).
    Always,
    /// Never color.
    Never,
}

/// How `search` orders what it prints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Sort {
    /// Agreement across sources and query coverage, best first.
    #[default]
    Relevance,
    /// Most recently updated first; undated results last.
    Newest,
}

// A macro rather than a const, because `concat!` takes only literals and the
// long help appends to the same block.
macro_rules! examples {
    () => {
        "Examples:
  dsk search sea surface temperature              every source, ranked
  dsk s mnist -s huggingface,kaggle               only these sources
  dsk s census --json | jq -r '.results[].url'    one field per line"
    };
}

#[derive(Parser)]
#[command(
    name = "dataseek",
    version,
    about = env!("CARGO_PKG_DESCRIPTION"),
    long_about = concat!(
        env!("CARGO_PKG_DESCRIPTION"),
        ".\n\ndsk is the same program under a shorter name."
    ),
    after_help = examples!(),
    after_long_help = concat!(
        examples!(),
        "\n\nExit status: 0 success, 1 failure, 2 usage error, 130 \
        interrupted.\n`dataseek help exit-codes` and `dataseek help \
        environment` say more.\n\nReport bugs at ",
        env!("CARGO_PKG_REPOSITORY"),
        "/issues"
    ),
    disable_help_subcommand = true,
    styles = palette::help()
)]
pub struct Cli {
    /// Errors only on stderr; stdout data is unaffected.
    #[arg(short = 'q', long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Explain what is happening on stderr.
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Emit JSON to stdout instead of text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Filter the JSON output through a jq expression; implies --json.
    #[arg(long, value_name = "EXPR", global = true)]
    pub jq: Option<String>,

    /// When to colorize output: auto, always, or never.
    #[arg(long, value_name = "WHEN", default_value = "auto", global = true)]
    pub color: ColorChoice,

    /// Disable ANSI colors; shorthand for --color=never (or set NO_COLOR).
    #[arg(long, global = true, hide_short_help = true)]
    pub no_color: bool,

    /// ASCII-only output; implies --color=never.
    #[arg(long, global = true)]
    pub plain: bool,

    /// No progress bars or spinners; status lines print plain, as in a pipe.
    #[arg(long, global = true, hide_short_help = true)]
    pub no_progress: bool,

    /// Keep the cache here instead of the platform cache directory.
    #[arg(
        long,
        value_name = "DIR",
        env = "DATASEEK_CACHE_DIR",
        global = true,
        hide_short_help = true
    )]
    pub cache_dir: Option<PathBuf>,

    /// Seconds a host gets to accept the connection; each request's own
    /// deadline (15 s, 90 s for catalogs) still caps it. A host that misses a
    /// limit you set is not marked as down.
    #[arg(
        long,
        value_name = "SECS",
        default_value_t = crate::http::DEFAULT_CONNECT_SECS,
        env = "DATASEEK_CONNECT_TIMEOUT",
        value_parser = clap::value_parser!(u64).range(1..),
        global = true,
        hide_short_help = true
    )]
    pub connect_timeout: u64,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Which sources to ask, shared by `search` and `bench`.
#[derive(Args, Clone, Debug, Default)]
pub struct Selection {
    /// Only these sources, comma-separated (see `dataseek sources`). Naming
    /// a source is the only way to ask an opt-in one.
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
        hide_possible_values = true,
        env = "DATASEEK_EXCLUDE"
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
        value_parser = clap::value_parser!(u16).range(1..=100),
        env = "DATASEEK_PER_SOURCE"
    )]
    pub per_source: u16,
}

fn source_ids() -> PossibleValuesParser {
    PossibleValuesParser::new(crate::sources::SOURCES.iter().map(|s| s.id))
}

#[derive(Subcommand)]
pub enum Command {
    /// Search every source at once, merged, deduplicated and ranked.
    #[command(
        visible_alias = "s",
        long_about = "Search every source at once, merged, deduplicated and \
            ranked.\n\nResults are ordered by relevance: how many sources \
            agree on a dataset and how much of the query it covers. \
            --sort newest orders them by their last update instead.",
        after_help = "Examples:
  dsk search unemployment -x google -n 50         skip a source, show more
  dsk search inflation -c economics,finance       only these categories
  dsk search census --jq '.results[].url'         one field, no jq needed"
    )]
    Search {
        /// What to look for; several words form one query.
        #[arg(required = true, value_name = "QUERY")]
        query: Vec<String>,

        #[command(flatten)]
        selection: Selection,

        /// How many merged results to print.
        #[arg(
            short = 'n',
            long,
            value_name = "N",
            default_value_t = 20,
            env = "DATASEEK_LIMIT"
        )]
        limit: usize,

        /// Order of the results.
        #[arg(long, value_name = "ORDER", default_value = "relevance")]
        sort: Sort,

        /// Ask every source again instead of reusing cached answers.
        #[arg(long, conflicts_with = "offline")]
        refresh: bool,

        /// Use only cached answers and downloaded catalogs; no network.
        #[arg(long)]
        offline: bool,

        /// Seconds to wait for slow sources before printing; 0 waits for all.
        #[arg(
            long,
            value_name = "SECS",
            default_value_t = 20,
            env = "DATASEEK_TIMEOUT"
        )]
        timeout: u64,
    },

    /// List every source: what it covers, its protocol, its key, its docs.
    Sources,

    /// Time every source on real queries and measure their overlap.
    #[command(
        long_about = "Time every source on real queries and measure their \
            overlap.\n\nBench bypasses the cache and asks every chosen source \
            once per query, so it sends queries x sources requests. Without \
            queries it uses a fixed set chosen to touch every category.",
        after_help = "Examples:
  dsk bench \"air quality\" \"protein structure\"     each query is one argument
  dsk bench mnist -s huggingface,kaggle,openml     only these sources"
    )]
    Bench {
        /// Queries to time; each is one argument (quote multi-word queries).
        #[arg(value_name = "QUERY")]
        queries: Vec<String>,

        #[command(flatten)]
        selection: Selection,
    },

    /// Read a dataset page's schema.org Dataset or Croissant metadata.
    #[command(after_help = "Examples:
  dsk inspect https://zenodo.org/records/1234567
  dsk inspect https://huggingface.co/datasets/stanfordnlp/imdb --json")]
    Inspect {
        /// The dataset's landing page.
        #[arg(value_name = "URL")]
        url: String,

        /// Fail at once instead of reaching for the network.
        #[arg(long)]
        offline: bool,
    },

    /// Serve search, sources and inspect to MCP clients over stdio.
    #[command(
        long_about = "Serve search, sources and inspect to MCP clients over \
            stdio.\n\nAn MCP client starts `dsk mcp` and speaks JSON-RPC on \
            its stdin and stdout. Each tool returns the object its command's \
            --json prints. stdout carries protocol messages only; narration \
            goes to stderr."
    )]
    Mcp,

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
    #[command(
        long_about = "Print the man page, in roff, to stdout.\n\nRead it \
            with `dataseek man | man -l -`, or install it with \
            `dataseek man > ~/.local/share/man/man1/dataseek.1`."
    )]
    Man,

    /// Explain a command or a topic: environment, exit-codes.
    #[command(long_about = "Explain a command or a topic: environment, \
        exit-codes.\n\nWith --json, describe every command, flag, \
        environment variable and exit code for scripts and agents.")]
    Help {
        /// A command name, `environment` or `exit-codes`.
        #[arg(value_name = "TOPIC", value_parser = crate::help::topics())]
        topic: Option<String>,
    },
}

#[derive(Subcommand, Clone, Copy)]
pub enum CacheAction {
    /// Where the cache lives, how full it is, and its budget.
    Info,
    /// Download every catalog-searched source's list now.
    Warm {
        /// List the catalogs that would be downloaded, and download none.
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete every cached result and catalog.
    Clear {
        /// Report what would be deleted, and delete nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};
    use proptest::prelude::*;

    use super::Cli;

    // Catches malformed definitions (duplicate short flags, bad defaults) that
    // are not compile errors.
    #[test]
    fn the_definition_is_well_formed() {
        Cli::command().debug_assert();
    }

    proptest! {
        // clap panics on some definition bugs only when a particular argv
        // reaches them; any argv must return, Ok or Err, without panicking.
        #[test]
        fn any_argv_parses_or_errors(
            args in prop::collection::vec(
                prop_oneof![
                    "[a-z-]{0,12}",
                    Just("--".to_owned()),
                    Just("-s".to_owned()),
                    Just("-n".to_owned()),
                    Just("--jq".to_owned()),
                    Just("--color".to_owned()),
                    Just("search".to_owned()),
                    Just("cache".to_owned()),
                    Just("help".to_owned()),
                    ".{0,8}",
                ],
                0..8,
            )
        ) {
            let _ = Cli::try_parse_from(
                std::iter::once("dataseek".to_owned()).chain(args),
            );
        }
    }
}
