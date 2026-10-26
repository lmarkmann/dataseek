//! The help surfaces clap does not draw: the grouped overview a bare
//! `dataseek` prints, the `help` topics (`environment`, `exit-codes`), and
//! `help --json`, the whole command surface as data for scripts and agents.

use std::any::TypeId;
use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use clap::builder::PossibleValuesParser;
use clap::builder::StyledStr;
use clap::{Arg, ArgAction};
use serde_json::{Value, json};

use crate::cli;
use crate::credentials::Key;
use crate::output::Out;
use crate::palette;

/// Every subcommand, as the `help` topic parser and the overview know them.
/// The parser runs while clap builds `Cli`, so it cannot ask `Cli` itself;
/// a test holds this list to the real one.
const COMMANDS: [&str; 10] = [
    "search",
    "sources",
    "inspect",
    "mcp",
    "bench",
    "cache",
    "doctor",
    "completion",
    "man",
    "help",
];

const TOPICS: [&str; 2] = ["environment", "exit-codes"];

/// The overview's groups. What each command does comes from its `about`, the
/// line `-h` shows, so the two cannot drift.
const GROUPS: [(&str, &[&str]); 3] = [
    ("Search", &["search", "sources", "inspect", "mcp"]),
    ("Upkeep", &["bench", "cache", "doctor"]),
    ("Shell", &["completion", "help"]),
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
    let cmd = cli::command();
    for (group, commands) in GROUPS {
        writeln!(w)?;
        writeln!(w, "{head}{group}:{head:#}")?;
        for sub in commands.iter().filter_map(|c| cmd.find_subcommand(c)) {
            let typed = std::iter::once(sub.get_name())
                .chain(sub.get_visible_aliases())
                .collect::<Vec<_>>()
                .join(", ");
            let about = sub.get_about().map(ToString::to_string);
            writeln!(
                w,
                "  {name}{typed:<11}{name:#} {}",
                about.unwrap_or_default()
            )?;
        }
    }
    writeln!(w)?;
    let bin = crate::invoked_name();
    writeln!(
        w,
        "{muted}run {bin} -h for a summary, {bin} --help for everything{muted:#}"
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
                "command": subcommand_json(command).unwrap_or(Value::Null),
            }),
        };
        return out.json(&surface);
    }
    let mut w = out.stdout();
    match topic {
        Some("environment") => {
            let vars = environment();
            let width = vars.keys().map(String::len).max().unwrap_or(0);
            let indent = width.saturating_add(4);
            let room = crate::output::room(indent);
            for (var, meaning) in &vars {
                let mut lines = crate::output::wrap(meaning, room).into_iter();
                let first = lines.next().unwrap_or_default();
                writeln!(w, "  {var:<width$}  {first}")?;
                for line in lines {
                    writeln!(w, "{:indent$}{line}", "")?;
                }
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
    let mut cmd = cli::command().bin_name(crate::invoked_name());
    cmd.build();
    cmd
}

/// Where `name completion <shell>` should go so the shell loads it, said
/// instead of printing the script to a terminal.
pub fn completion_hint(shell: clap_complete::Shell, name: &str) -> String {
    use clap_complete::Shell;

    let save = match shell {
        Shell::Bash => {
            format!("> ~/.local/share/bash-completion/completions/{name}")
        }
        Shell::Fish => format!("> ~/.config/fish/completions/{name}.fish"),
        Shell::Zsh => format!(
            "> ~/.zfunc/_{name}   (with ~/.zfunc on $fpath before compinit)"
        ),
        Shell::Elvish => ">> ~/.config/elvish/rc.elv".to_owned(),
        Shell::PowerShell => ">> $PROFILE".to_owned(),
        _ => format!("> a file {shell} loads at startup"),
    };
    format!(
        "the script is for a file, not the terminal; save it with: \
         {name} completion {shell} {save}"
    )
}

/// Show `page` through `man`, as a reader on a terminal expects. `man` takes a
/// file path everywhere (BSD and macOS `man` have no `-l`), so the page goes
/// through a temporary file: a random name, created only if absent and
/// readable by this user alone, so a link planted in a shared temp directory
/// cannot redirect the write, and gone when the guard drops. False when `man`
/// is missing or fails, so the caller prints the roff instead.
pub fn show_man(page: &[u8]) -> bool {
    let Ok(mut file) =
        tempfile::Builder::new().prefix("dataseek-").suffix(".1").tempfile()
    else {
        return false;
    };
    file.write_all(page).is_ok()
        && file.flush().is_ok()
        && std::process::Command::new("man")
            .arg(file.path())
            .status()
            .is_ok_and(|status| status.success())
}

/// The man page, in roff, drawn from the plain definition, because roff
/// would run together the value lists that `cli::command()` writes into the
/// long help. A flag hidden from `-h` has no long help of its own, so its
/// help stands in. The examples, exit codes and bug address that `--help`
/// closes with become EXAMPLES, EXIT STATUS and REPORTING BUGS, from the
/// same examples and from [`EXIT_CODES`].
pub fn man_page() -> String {
    use std::fmt::Write as _;

    use clap::CommandFactory;

    let mut cmd = cli::Cli::command();
    cmd.build();
    let name = cmd.get_name();
    let version = env!("CARGO_PKG_VERSION");
    let about = cmd.get_about().map(ToString::to_string).unwrap_or_default();
    let mut page = format!(
        ".ie \\n(.g .ds Aq \\(aq\n.el .ds Aq '\n\
         .TH {name} 1 \"\" \"{name} {version}\"\n\
         .SH NAME\n{name} \\- {}\n.SH SYNOPSIS\n\\fB{name}\\fR",
        roff(&about)
    );
    let options: Vec<&Arg> = cmd
        .get_arguments()
        .filter(|arg| !arg.is_hide_set() && !arg.is_positional())
        .collect();
    for arg in &options {
        let repeats = matches!(arg.get_action(), ArgAction::Count);
        let _ = write!(
            page,
            " [{}]{}",
            flags(arg, "|"),
            if repeats { "..." } else { "" }
        );
    }
    page.push_str(" [\\fIcommand\\fR]\n.SH DESCRIPTION\n");
    let description = cmd.get_long_about().map(ToString::to_string);
    page.push_str(&lines(&description.unwrap_or(about), ".PP"));
    page.push_str(".SH OPTIONS\n");
    for arg in &options {
        let _ = write!(page, ".TP\n{}", flags(arg, ", "));
        if arg.get_action().takes_values() {
            for value in arg.get_value_names().unwrap_or_default() {
                let _ = write!(page, " \\fI<{}>\\fR", roff(value.as_str()));
            }
            let defaults: Vec<_> = arg
                .get_default_values()
                .iter()
                .map(|value| value.to_string_lossy())
                .collect();
            if !defaults.is_empty() && !arg.is_hide_default_value_set() {
                let _ =
                    write!(page, " [default: {}]", roff(&defaults.join(",")));
            }
        }
        let help = arg.get_long_help().or_else(|| arg.get_help());
        let help = help.map(ToString::to_string).unwrap_or_default();
        let _ = write!(page, "\n{}", lines(&help, ".br"));
        let values: Vec<_> = arg
            .get_possible_values()
            .into_iter()
            .filter(|value| !value.is_hide_set())
            .collect();
        if !values.is_empty() && !arg.is_hide_possible_values_set() {
            page.push_str(".br\n\\fIPossible values:\\fR\n.RS 14\n");
            for value in values {
                let meaning = value.get_help().map(|help| format!(": {help}"));
                let item = format!(
                    "{}{}",
                    value.get_name(),
                    meaning.unwrap_or_default()
                );
                let _ = writeln!(page, ".IP \\(bu 2\n{}", roff(&item));
            }
            page.push_str(".RE\n");
        }
    }
    page.push_str(".SH COMMANDS\n");
    for sub in cmd.get_subcommands().filter(|sub| !sub.is_hide_set()) {
        let about = sub.get_about().map(ToString::to_string);
        let _ = writeln!(
            page,
            ".TP\n\\fB{}\\fR\n{}",
            roff(sub.get_name()),
            roff(&about.unwrap_or_default())
        );
    }
    page.push_str(".SH EXAMPLES\n.nf\n");
    for line in cli::examples!().lines().skip(1) {
        let _ = writeln!(page, "{}", roff(line));
    }
    page.push_str(".fi\n.SH \"EXIT STATUS\"\n");
    for (code, meaning) in EXIT_CODES {
        let _ = writeln!(page, ".TP\n\\fB{code}\\fR\n{}", roff(meaning));
    }
    let issues = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");
    let _ = writeln!(page, ".SH \"REPORTING BUGS\"\n{}", roff(issues));
    let _ = writeln!(page, ".SH VERSION\nv{version}");
    page
}

/// An option's flags in bold, `-q` before `--quiet`, joined by `separator`.
fn flags(arg: &Arg, separator: &str) -> String {
    let short = arg.get_short().map(|c| format!("-{c}"));
    let long = arg.get_long().map(|long| format!("--{long}"));
    short
        .into_iter()
        .chain(long)
        .map(|flag| format!("\\fB{}\\fR", roff(&flag)))
        .collect::<Vec<_>>()
        .join(separator)
}

/// Each line of `text` as a line of roff, a blank one as the `blank`
/// request.
fn lines(text: &str, blank: &str) -> String {
    text.lines()
        .map(|line| match line.trim() {
            "" => format!("{blank}\n"),
            _ => format!("{}\n", roff(line)),
        })
        .collect()
}

/// `text` as roff text: backslashes, hyphens and apostrophes escaped, and a
/// leading period kept from reading as a request.
fn roff(text: &str) -> String {
    let escaped =
        text.replace('\\', "\\e").replace('-', "\\-").replace('\'', "\\*(Aq");
    if escaped.starts_with('.') { format!("\\&{escaped}") } else { escaped }
}

/// clap's styled help as ANSI text; the stream decides whether it stays.
fn ansi(help: &StyledStr) -> String {
    help.ansi().to_string()
}

/// The variables that change what dataseek does, with what each does. The `DATASEEK_*` ones
/// come from the flag definitions, the keys from the key registry.
fn environment() -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    let mut commands = vec![cli::command()];
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
    vars.insert(
        crate::cache::BUDGET_VAR.to_owned(),
        "cache size budget in whole megabytes, 1 to 10000; 30 by default"
            .to_owned(),
    );
    for (var, meaning) in [
        ("KAGGLE_CONFIG_DIR", "where the Kaggle CLI keeps its token"),
        ("NO_COLOR", "any non-empty value turns color off"),
        ("CLICOLOR_FORCE", "1 keeps color on, even through a pipe"),
        (
            "ALL_PROXY",
            "proxy for every request; else HTTPS_PROXY, then HTTP_PROXY",
        ),
        ("NO_PROXY", "hosts that skip the proxy"),
        (
            "SSL_CERT_FILE",
            "Linux: PEM file of root certificates trusted instead of the system's",
        ),
        (
            "SSL_CERT_DIR",
            "Linux: directories of root certificates trusted instead of the system's",
        ),
        (
            "XDG_CONFIG_HOME",
            "parent of the config directory (credentials.toml)",
        ),
        ("XDG_CACHE_HOME", "parent of the cache directory"),
        ("XDG_STATE_HOME", "parent of the state directory"),
        ("COLUMNS", "line width for results; for help only off a terminal"),
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
        "command": command_json(&cli::command()),
        "environment": variables(),
        "exit_codes": codes(),
    })
}

/// One command as `help <command> --json` describes it; `mcp` builds its
/// tool schemas from the same object.
pub fn subcommand_json(name: &str) -> Option<Value> {
    cli::command().find_subcommand(name).map(command_json)
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
        "commands": cmd.get_subcommands().map(command_json)
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
    let integer =
        [TypeId::of::<u16>(), TypeId::of::<u64>(), TypeId::of::<usize>()]
            .iter()
            .any(|id| arg.get_value_parser().type_id() == *id);
    let kind = match arg.get_action() {
        ArgAction::Count => "count",
        _ if flag => "boolean",
        _ if integer => "integer",
        _ => "string",
    };
    let (minimum, maximum) = bounds(arg);
    let strings = |items: &[&std::ffi::OsStr]| -> Vec<String> {
        items.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    };
    json!({
        "name": arg.get_id().as_str(),
        "long": arg.get_long(),
        "short": arg.get_short().map(String::from),
        "positional": arg.is_positional(),
        "type": kind,
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
        "minimum": minimum,
        "maximum": maximum,
        "env": arg.get_env().map(|e| e.to_string_lossy().into_owned()),
        "required": arg.is_required_set(),
        "repeatable": matches!(arg.get_action(), ArgAction::Append | ArgAction::Count),
        "global": arg.is_global_set(),
        "help": arg.get_help().map(ToString::to_string),
    })
}

/// The bounds an integer flag's parser enforces. clap keeps a range inside
/// the parser, where nothing can read it back, so each ranged flag is named
/// here; a test holds this list to the parsers.
fn bounds(arg: &Arg) -> (Value, Value) {
    match arg.get_id().as_str() {
        "connect_timeout" => (json!(cli::CONNECT_TIMEOUT.start), Value::Null),
        "per_source" => {
            (json!(cli::PER_SOURCE.start()), json!(cli::PER_SOURCE.end()))
        }
        "min_sources" => (json!(cli::MIN_SOURCES.start), Value::Null),
        _ => (Value::Null, Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;

    use super::*;

    fn subcommands() -> Vec<String> {
        cli::command()
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
    fn the_overview_shows_every_command_in_help_order() {
        let shown: Vec<&str> = GROUPS
            .iter()
            .flat_map(|(_, commands)| commands.iter().copied())
            .collect();
        let cmd = cli::command();
        let listed: Vec<&str> = cmd
            .get_subcommands()
            .filter(|c| !c.is_hide_set())
            .map(clap::Command::get_name)
            .collect();
        assert_eq!(shown, listed);
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

    // Every integer flag takes its published bounds and refuses the values
    // just past them; one with no bound published takes 0, or 65535.
    #[test]
    fn published_bounds_are_the_ones_clap_enforces() {
        let root = command_json(&cli::command());
        let leaves = root["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["commands"] == json!([]))
            .map(|c| (c["name"].as_str().unwrap(), &c["args"]));
        let mut checked = Vec::new();
        for (name, args) in leaves.chain([("sources", &root["args"])]) {
            let args = args.as_array().unwrap();
            let words: Vec<&str> = args
                .iter()
                .filter(|a| a["positional"] == true && a["required"] == true)
                .map(|_| "x")
                .collect();
            for arg in args.iter().filter(|a| a["type"] == "integer") {
                let long = arg["long"].as_str().unwrap();
                let takes = |n: i64| {
                    let flag = format!("--{long}={n}");
                    let line = ["dataseek", name, flag.as_str()];
                    Cli::try_parse_from(line.into_iter().chain(words.clone()))
                        .is_ok()
                };
                let (min, max) =
                    (arg["minimum"].as_i64(), arg["maximum"].as_i64());
                let low = min.unwrap_or(0);
                let high = max.unwrap_or(i64::from(u16::MAX));
                assert!(takes(low) && takes(high), "{name} --{long}");
                if let Some(min) = min {
                    assert!(!takes(min.checked_sub(1).unwrap()), "--{long}");
                }
                if let Some(max) = max {
                    assert!(!takes(max.checked_add(1).unwrap()), "--{long}");
                }
                checked.push(long);
            }
        }
        assert!(checked.contains(&"per-source"), "{checked:?}");
        assert!(checked.contains(&"connect-timeout"), "{checked:?}");
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
        let kind = |long: &str| {
            search["args"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["long"] == long)
                .unwrap()["type"]
                .clone()
        };
        assert_eq!(kind("limit"), "integer");
        assert_eq!(kind("per-source"), "integer");
        assert_eq!(kind("sort"), "string");
        assert_eq!(kind("offline"), "boolean");
        assert_eq!(surface["exit_codes"].as_array().unwrap().len(), 4);
    }
}
