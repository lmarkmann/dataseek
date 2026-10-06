//! The help surfaces clap does not draw: the grouped overview a bare
//! `dataseek` prints, the `help` topics (`environment`, `exit-codes`), and
//! `help --json`, the whole command surface as data for scripts and agents.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use clap::builder::PossibleValuesParser;
use clap::builder::StyledStr;
use clap::{Arg, ArgAction, CommandFactory};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::credentials::Key;
use crate::output::Out;
use crate::palette;

/// Every subcommand, as the `help` topic parser and the overview know them.
/// The parser runs while clap builds `Cli`, so it cannot ask `Cli` itself;
/// a test holds this list to the real one.
const COMMANDS: [&str; 9] = [
    "search",
    "sources",
    "bench",
    "inspect",
    "cache",
    "doctor",
    "completion",
    "man",
    "help",
];

const TOPICS: [&str; 2] = ["environment", "exit-codes"];

/// The overview's groups: the name to type and a few words on what it does.
const GROUPS: [(&str, &[(&str, &str)]); 3] = [
    (
        "search",
        &[
            ("search, s", "search every source at once"),
            ("sources", "list sources and their keys"),
            ("inspect", "read a dataset page's metadata"),
            ("bench", "time and compare sources"),
        ],
    ),
    (
        "upkeep",
        &[
            ("cache", "show, warm or clear the cache"),
            ("doctor", "check keys, paths and cache"),
        ],
    ),
    (
        "shell",
        &[
            ("completion", "print a completion script"),
            ("man", "print the man page"),
            ("help", "explain a command or topic"),
        ],
    ),
];

/// Every code the binary returns. `help exit-codes`, `help --json` and the
/// test that asserts each one all read this.
pub const EXIT_CODES: [(u8, &str); 4] = [
    (
        0,
        "success; for search, at least one source answered, even with no results",
    ),
    (
        1,
        "failure: an error, every source failed or none could run, or a catalog failed to download",
    ),
    (2, "usage error: an unknown command, flag or value"),
    (130, "interrupted by Ctrl-C"),
];

pub fn topics() -> PossibleValuesParser {
    PossibleValuesParser::new(COMMANDS.into_iter().chain(TOPICS))
}

/// The orientation screen for a bare invocation: grouped, short, on stdout.
pub fn overview(out: &Out) -> Result<()> {
    let mut w = out.stdout();
    let (head, name, muted) =
        (palette::accent(), palette::success(), palette::muted());
    let glyph = if out.plain { "*" } else { "\u{25c6}" };
    writeln!(
        w,
        "{head}{glyph} {}{head:#} {muted}v{}{muted:#}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )?;
    for (group, commands) in GROUPS {
        writeln!(w)?;
        writeln!(w, "{head}{group}{head:#}")?;
        for (command, about) in commands {
            writeln!(w, "  {name}{command:<11}{name:#} {about}")?;
        }
    }
    writeln!(w)?;
    writeln!(
        w,
        "{muted}run {} -h for full usage{muted:#}",
        crate::invoked_name()
    )?;
    Ok(())
}

/// `help`, `help <command>`, `help environment`, `help exit-codes`, and the
/// same with `--json`.
pub fn run(topic: Option<&str>, out: &Out) -> Result<()> {
    if out.json {
        let surface = match topic {
            None => surface(),
            Some("environment") => env_json(),
            Some("exit-codes") => codes_json(),
            Some(command) => json!({
                "schema": "dataseek-command/1",
                "command": built()
                    .find_subcommand(command)
                    .map_or(Value::Null, command_json),
            }),
        };
        return out.json(&surface);
    }
    let mut w = out.stdout();
    match topic {
        Some("environment") => {
            let vars = environment();
            let width = vars.keys().map(String::len).max().unwrap_or(0);
            for (var, meaning) in &vars {
                writeln!(w, "  {var:<width$}  {meaning}")?;
            }
        }
        Some("exit-codes") => {
            for (code, meaning) in EXIT_CODES {
                writeln!(w, "  {code:<3}  {meaning}")?;
            }
        }
        Some(command) => {
            let mut cmd = built();
            if let Some(sub) = cmd.find_subcommand_mut(command) {
                write!(w, "{}", ansi(&sub.render_long_help()))?;
            }
        }
        None => write!(w, "{}", ansi(&built().render_long_help()))?,
    }
    Ok(())
}

/// The command, built so subcommand usage lines carry the invoked name.
fn built() -> clap::Command {
    let mut cmd = Cli::command().bin_name(crate::invoked_name());
    cmd.build();
    cmd
}

/// clap's styled help as ANSI text; the stream decides whether it stays.
fn ansi(help: &StyledStr) -> String {
    help.ansi().to_string()
}

/// Every variable dataseek reads, with what it does. The `DATASEEK_*` ones
/// come from the flag definitions, the keys from the key registry.
fn environment() -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    let mut commands = vec![Cli::command()];
    while let Some(cmd) = commands.pop() {
        for arg in cmd.get_arguments() {
            if let (Some(env), Some(long)) = (arg.get_env(), arg.get_long()) {
                let help = arg.get_help().map(ToString::to_string);
                vars.insert(
                    env.to_string_lossy().into_owned(),
                    format!("--{long}: {}", help.unwrap_or_default()),
                );
            }
        }
        commands.extend(cmd.get_subcommands().cloned());
    }
    for key in Key::ALL {
        vars.insert(
            key.env_var().to_owned(),
            format!("API key, get one at {}", key.signup()),
        );
    }
    for (var, meaning) in [
        ("KAGGLE_CONFIG_DIR", "where the Kaggle CLI keeps its token"),
        ("NO_COLOR", "any non-empty value turns color off"),
        ("CLICOLOR_FORCE", "1 keeps color on, even through a pipe"),
        (
            "HTTPS_PROXY",
            "proxy for every request (also HTTP_PROXY, ALL_PROXY)",
        ),
        ("NO_PROXY", "hosts that skip the proxy"),
        (
            "XDG_CONFIG_HOME",
            "parent of the config directory (credentials.toml)",
        ),
        ("XDG_CACHE_HOME", "parent of the cache directory"),
        ("XDG_STATE_HOME", "parent of the state directory"),
        ("COLUMNS", "line width for help and results"),
    ] {
        vars.insert(var.to_owned(), meaning.to_owned());
    }
    vars
}

fn variables() -> Vec<Value> {
    environment()
        .into_iter()
        .map(|(name, meaning)| json!({ "name": name, "meaning": meaning }))
        .collect()
}

fn codes() -> Vec<Value> {
    EXIT_CODES
        .iter()
        .map(|(code, meaning)| json!({ "code": code, "meaning": meaning }))
        .collect()
}

fn env_json() -> Value {
    json!({ "schema": "dataseek-environment/1", "variables": variables() })
}

fn codes_json() -> Value {
    json!({ "schema": "dataseek-exit-codes/1", "codes": codes() })
}

/// The whole surface: commands, flags, values, defaults, variables, codes.
fn surface() -> Value {
    json!({
        "schema": "dataseek-surface/1",
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "command": command_json(&Cli::command()),
        "environment": variables(),
        "exit_codes": codes(),
    })
}

fn command_json(cmd: &clap::Command) -> Value {
    json!({
        "name": cmd.get_name(),
        "about": cmd.get_about().map(ToString::to_string),
        "aliases": cmd.get_visible_aliases().collect::<Vec<_>>(),
        "args": cmd
            .get_arguments()
            .filter(|a| !a.is_hide_set())
            .map(arg_json)
            .collect::<Vec<_>>(),
        "commands": cmd
            .get_subcommands()
            .filter(|c| !c.is_hide_set())
            .map(command_json)
            .collect::<Vec<_>>(),
    })
}

fn arg_json(arg: &Arg) -> Value {
    let flag = matches!(
        arg.get_action(),
        ArgAction::SetTrue
            | ArgAction::SetFalse
            | ArgAction::Count
            | ArgAction::Help
            | ArgAction::HelpShort
            | ArgAction::HelpLong
            | ArgAction::Version
    );
    let strings = |items: &[&std::ffi::OsStr]| -> Vec<String> {
        items.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    };
    json!({
        "name": arg.get_id().as_str(),
        "long": arg.get_long(),
        "short": arg.get_short().map(String::from),
        "positional": arg.is_positional(),
        "value": (!flag).then(|| {
            arg.get_value_names()
                .map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>().join(" "))
        }),
        "values": arg
            .get_possible_values()
            .iter()
            .map(|v| v.get_name().to_owned())
            .collect::<Vec<_>>(),
        "default": strings(&arg.get_default_values().iter().map(AsRef::as_ref).collect::<Vec<_>>()),
        "env": arg.get_env().map(|e| e.to_string_lossy().into_owned()),
        "required": arg.is_required_set(),
        "repeatable": matches!(arg.get_action(), ArgAction::Append | ArgAction::Count),
        "global": arg.is_global_set(),
        "help": arg.get_help().map(ToString::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subcommands() -> Vec<String> {
        Cli::command()
            .get_subcommands()
            .map(|c| c.get_name().to_owned())
            .collect()
    }

    #[test]
    fn the_topic_list_names_every_command() {
        assert_eq!(subcommands(), COMMANDS);
    }

    // A command missing from the overview is a command nobody finds.
    #[test]
    fn the_overview_shows_every_command() {
        let shown: Vec<&str> = GROUPS
            .iter()
            .flat_map(|(_, commands)| commands.iter())
            .map(|(name, _)| name.split(',').next().unwrap_or(name))
            .collect();
        for command in subcommands() {
            assert!(shown.contains(&command.as_str()), "{command} missing");
        }
    }

    #[test]
    fn groups_stay_small() {
        for (group, commands) in GROUPS {
            assert!(commands.len() < 7, "{group} has {}", commands.len());
        }
    }

    #[test]
    fn the_environment_lists_flags_keys_and_general_variables() {
        let vars = environment();
        for var in ["DATASEEK_EXCLUDE", "DATASEEK_CACHE_DIR", "NO_COLOR"] {
            assert!(vars.contains_key(var), "{var} missing");
        }
        for key in Key::ALL {
            assert!(vars.contains_key(key.env_var()));
        }
    }

    #[test]
    fn the_surface_describes_flags_with_types_and_defaults() {
        let surface = surface();
        let search = surface["command"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "search")
            .unwrap();
        let limit = search["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["long"] == "limit")
            .unwrap();
        assert_eq!(limit["short"], "n");
        assert_eq!(limit["default"], json!(["20"]));
        assert_eq!(limit["env"], "DATASEEK_LIMIT");
        assert_eq!(surface["exit_codes"].as_array().unwrap().len(), 4);
    }
}
