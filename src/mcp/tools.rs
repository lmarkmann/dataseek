//! The three tools. Each one's input schema is generated from the object
//! `help <command> --json` prints, and a call's arguments become that
//! command's flags through the same object, so a tool and its command cannot
//! drift apart: a flag added to `search` is a `search` argument at once, and
//! clap validates every value exactly as it does on the command line.
//!
//! A call runs this binary's own command with `--json` in a child process.
//! Its stdout is the command's JSON, byte for byte; its stderr events are
//! relayed to ours, and the error event of a failed run becomes the tool's
//! error result. The child also takes `--offline`, the search loop's detached
//! threads and every other process global with it when it exits.

use std::io::{self, BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{Failure, help, ui};

/// The tools, in the order `tools/list` returns them.
pub const TOOLS: [&str; 3] = ["search", "sources", "inspect"];

/// The global flags `dsk mcp` was started with that its children inherit.
pub struct Globals {
    pub quiet: bool,
    pub verbose: u8,
    pub cache_dir: Option<PathBuf>,
    pub connect_timeout: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "{tool} has no argument `{key}`\n  Try:   tools/list shows the arguments it takes"
    )]
    Unknown { tool: &'static str, key: String },
    #[error(
        "`{key}` takes {expected}\n  Try:   tools/list shows the type of each argument"
    )]
    Type { key: String, expected: &'static str },
    #[error(
        "timeout 0 waits for every source, and a search over MCP always has a deadline\n  Try:   pass a timeout of 1 or more seconds, or leave it out for the default"
    )]
    NoDeadline,
}

/// A command as `help <command> --json` describes it, keeping the keys the
/// schema needs.
#[derive(Deserialize)]
struct Described {
    about: Option<String>,
    args: Vec<Arg>,
}

/// The value an argument takes, as `help --json` names it. A kind this
/// list lacks fails the tool list instead of becoming a string.
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ValueKind {
    String,
    Integer,
    Boolean,
    /// A flag given n times, such as `-vv`.
    Count,
}

#[derive(Deserialize)]
struct Arg {
    name: String,
    long: Option<String>,
    positional: bool,
    #[serde(rename = "type")]
    kind: ValueKind,
    values: Vec<String>,
    default: Vec<String>,
    required: bool,
    repeatable: bool,
    help: Option<String>,
}

impl Arg {
    /// The argument's name in the tool: the long flag, or a positional's id.
    fn key(&self) -> &str {
        self.long.as_deref().unwrap_or(&self.name)
    }

    /// Positionals are joined into one string (`search`'s words form one
    /// query), and a count is how often its flag is repeated, so only named
    /// flags of other kinds take lists.
    fn takes_list(&self) -> bool {
        self.repeatable && !self.positional && self.kind != ValueKind::Count
    }

    fn expected(&self) -> &'static str {
        match (self.takes_list(), self.kind) {
            (true, ValueKind::Integer) => "a list of integers",
            (true, _) => "a list of strings",
            (false, ValueKind::Boolean) => "true or false",
            (false, ValueKind::Integer | ValueKind::Count) => "an integer",
            (false, ValueKind::String) => "a string",
        }
    }
}

fn described(tool: &str) -> anyhow::Result<Described> {
    let command = help::subcommand_json(tool)
        .ok_or_else(|| anyhow::anyhow!("no `{tool}` command to describe"))?;
    Ok(serde_json::from_value(command)?)
}

/// The tool whose name is `name`, as the static string the rest uses.
pub fn find(name: &str) -> Option<&'static str> {
    TOOLS.into_iter().find(|tool| *tool == name)
}

/// What `tools/list` returns.
pub fn definitions() -> anyhow::Result<Vec<Value>> {
    TOOLS
        .iter()
        .map(|tool| {
            let command = described(tool)?;
            Ok(json!({
                "name": tool,
                "description": command.about,
                "inputSchema": input_schema(&command.args),
            }))
        })
        .collect()
}

fn input_schema(args: &[Arg]) -> Value {
    let named = || args.iter().filter(|a| a.long.is_some() || a.positional);
    let properties: Map<String, Value> =
        named().map(|arg| (arg.key().to_owned(), property(arg))).collect();
    let required: Vec<&str> =
        named().filter(|a| a.required).map(Arg::key).collect();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// One argument's schema: its type and possible values, for each item when
/// it takes a list, then its help and default.
fn property(arg: &Arg) -> Value {
    let kind = match arg.kind {
        ValueKind::String => "string",
        ValueKind::Integer | ValueKind::Count => "integer",
        ValueKind::Boolean => "boolean",
    };
    let mut value = Map::new();
    value.insert("type".into(), json!(kind));
    // clap lists a boolean flag's values as the strings "true" and "false",
    // which a boolean can never equal.
    if arg.kind == ValueKind::String && !arg.values.is_empty() {
        value.insert("enum".into(), json!(arg.values));
    }
    let mut property = if arg.takes_list() {
        let mut list = Map::new();
        list.insert("type".into(), json!("array"));
        list.insert("items".into(), Value::Object(value));
        list
    } else {
        value
    };
    if let Some(help) = &arg.help {
        property.insert("description".into(), json!(help));
    }
    let mut defaults = arg.default.iter().map(|text| typed(arg.kind, text));
    let default = if arg.takes_list() {
        Some(Value::Array(defaults.collect())).filter(|d| d != &json!([]))
    } else {
        defaults.next()
    };
    if let Some(default) = default {
        property.insert("default".into(), default);
    }
    Value::Object(property)
}

/// A default from `help --json`, which lists every default as text, in the
/// argument's own type.
fn typed(kind: ValueKind, text: &str) -> Value {
    match kind {
        ValueKind::Integer | ValueKind::Count => {
            text.parse::<u64>().map_or_else(|_| json!(text), Value::from)
        }
        ValueKind::Boolean => {
            text.parse::<bool>().map_or_else(|_| json!(text), Value::from)
        }
        ValueKind::String => json!(text),
    }
}

/// The command line a call of `tool` with `arguments` runs, after the
/// program name. Values are attached with `=`, and the positional comes
/// after `--`, so no value can be read as a flag.
pub fn argv(
    tool: &'static str,
    arguments: &Map<String, Value>,
) -> anyhow::Result<Vec<String>> {
    let flags = described(tool)?.args;
    let mut argv = vec![tool.to_owned(), "--json".to_owned()];
    let mut positional = Vec::new();
    for (key, value) in arguments {
        let arg = flags
            .iter()
            .find(|a| a.key() == key && (a.long.is_some() || a.positional))
            .ok_or_else(|| Error::Unknown { tool, key: key.clone() })?;
        let flag = format!("--{key}");
        match (arg.kind, value) {
            _ if arg.positional => positional.push(scalar(arg, value)?),
            (ValueKind::Boolean, Value::Bool(true)) => argv.push(flag),
            (ValueKind::Boolean, Value::Bool(false)) => {}
            (ValueKind::Count, Value::Number(n)) => {
                let times = n
                    .as_u64()
                    .and_then(|times| u8::try_from(times).ok())
                    .ok_or_else(|| Error::Type {
                        key: key.clone(),
                        expected: arg.expected(),
                    })?;
                argv.extend((0..times).map(|_| flag.clone()));
            }
            (_, Value::Array(items)) if arg.takes_list() => {
                for item in items {
                    argv.push(format!("{flag}={}", scalar(arg, item)?));
                }
            }
            _ if arg.takes_list() => {
                return Err(Error::Type {
                    key: key.clone(),
                    expected: arg.expected(),
                }
                .into());
            }
            (_, other) => {
                argv.push(format!("{flag}={}", scalar(arg, other)?));
            }
        }
    }
    if tool == "search" {
        argv.extend(deadline(&flags, arguments)?);
    }
    if !positional.is_empty() {
        argv.push("--".to_owned());
        argv.extend(positional);
    }
    Ok(argv)
}

/// ADR 0007's deadline, made explicit for every search: the flag beats an
/// inherited `DATASEEK_TIMEOUT`, and 0, which waits for all, is refused.
fn deadline(
    args: &[Arg],
    arguments: &Map<String, Value>,
) -> Result<Option<String>, Error> {
    match arguments.get("timeout") {
        Some(given) if given.as_u64() == Some(0) => Err(Error::NoDeadline),
        Some(_) => Ok(None),
        None => Ok(args
            .iter()
            .find(|a| a.long.as_deref() == Some("timeout"))
            .and_then(|a| a.default.first())
            .map(|secs| format!("--timeout={secs}"))),
    }
}

fn scalar(arg: &Arg, value: &Value) -> Result<String, Error> {
    match (arg.kind, value) {
        (ValueKind::String, Value::String(text)) => Ok(text.clone()),
        (ValueKind::Integer, Value::Number(n)) if n.is_u64() => {
            Ok(n.to_string())
        }
        _ => Err(Error::Type {
            key: arg.key().to_owned(),
            expected: arg.expected(),
        }),
    }
}

/// Start `argv` as a child of this binary, with the globals `dsk mcp` was
/// given; its stdin is closed, its stdout and stderr piped back.
pub fn spawn(argv: &[String], globals: &Globals) -> io::Result<Child> {
    let mut command = Command::new(std::env::current_exe()?);
    if globals.quiet {
        command.arg("--quiet");
    }
    for _ in 0..globals.verbose {
        command.arg("--verbose");
    }
    if let Some(dir) = &globals.cache_dir {
        command.env("DATASEEK_CACHE_DIR", dir);
    }
    command
        .env("DATASEEK_CONNECT_TIMEOUT", globals.connect_timeout.to_string())
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

/// What a child printed: its whole stdout, and the last error event on its
/// stderr, every line of which has already been relayed to ours.
pub struct Output {
    pub stdout: Vec<u8>,
    pub error: Option<Value>,
}

/// Read a child's two pipes to their ends, relaying its stderr as it comes.
pub fn collect(
    stdout: Option<impl Read>,
    stderr: Option<ChildStderr>,
) -> Output {
    std::thread::scope(|scope| {
        let relay = scope.spawn(|| {
            let mut error = None;
            for line in
                stderr.into_iter().flat_map(|s| BufReader::new(s).lines())
            {
                let Ok(line) = line else { break };
                ui::relay(&line);
                if let Ok(event) = serde_json::from_str::<Value>(&line)
                    && event.get("event") == Some(&json!("error"))
                {
                    error = Some(event);
                }
            }
            error
        });
        let mut bytes = Vec::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut bytes);
        }
        Output { stdout: bytes, error: relay.join().ok().flatten() }
    })
}

/// The `tools/call` result for a child that exited with `status`, or `None`
/// when the run left nothing to report, which is a server error.
pub fn result(status: ExitStatus, output: &Output) -> Option<Value> {
    if status.success() {
        let text =
            String::from_utf8_lossy(&output.stdout).trim_end().to_owned();
        let structured: Value = serde_json::from_str(&text).ok()?;
        return Some(json!({
            "content": [{ "type": "text", "text": text }],
            "structuredContent": structured,
            "isError": false,
        }));
    }
    let event = output.error.as_ref()?;
    Some(failed(&Failure::from_event(event)?))
}

/// A failed call as a result the model can read and act on.
pub fn failed(failure: &Failure) -> Value {
    let mut text = Vec::new();
    let _ = failure.write(&mut text, clap::builder::styling::Style::new());
    json!({
        "content": [{
            "type": "text",
            "text": String::from_utf8_lossy(&text).trim_end(),
        }],
        "structuredContent": failure.event(),
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command as Cmd, Sort};

    fn arguments(value: Value) -> Map<String, Value> {
        let Value::Object(fields) = value else { panic!("not an object") };
        fields
    }

    fn schema(tool: &str) -> Value {
        definitions().unwrap().into_iter().find(|t| t["name"] == tool).unwrap()
            ["inputSchema"]
            .clone()
    }

    // The schema is the command's own description, flag for flag.
    #[test]
    fn every_tool_takes_exactly_its_commands_flags() {
        for tool in TOOLS {
            let command = help::subcommand_json(tool).unwrap();
            let mut flags: Vec<String> = command["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    a["long"]
                        .as_str()
                        .or(a["name"].as_str())
                        .unwrap()
                        .to_owned()
                })
                .collect();
            let mut properties: Vec<String> = schema(tool)["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            flags.sort();
            properties.sort();
            assert_eq!(properties, flags, "{tool}");
        }
    }

    #[test]
    fn the_search_schema_carries_types_values_and_defaults() {
        let search = schema("search");
        let p = &search["properties"];
        assert_eq!(p["query"]["type"], "string");
        assert_eq!(p["limit"]["type"], "integer");
        assert_eq!(p["limit"]["default"], 20);
        assert_eq!(p["offline"]["type"], "boolean");
        assert_eq!(p["offline"].get("enum"), None);
        assert_eq!(p["refresh"].get("enum"), None);
        assert_eq!(p["sort"]["enum"], json!(["relevance", "newest"]));
        assert_eq!(p["source"]["type"], "array");
        assert!(
            p["source"]["items"]["enum"]
                .as_array()
                .unwrap()
                .contains(&json!("zenodo"))
        );
        assert_eq!(search["required"], json!(["query"]));
        assert_eq!(search["additionalProperties"], false);
    }

    // Through clap, the generated command line means what the call said.
    #[test]
    fn a_call_becomes_the_command_line_it_describes() {
        let argv = argv(
            "search",
            &arguments(json!({
                "query": "sea surface temperature",
                "source": ["zenodo", "datacite"],
                "limit": 5,
                "sort": "newest",
                "offline": true,
                "refresh": false,
            })),
        )
        .unwrap();
        let cli =
            Cli::try_parse_from(std::iter::once("dsk".to_owned()).chain(argv))
                .unwrap();
        assert!(cli.json);
        let Some(Cmd::Search {
            query,
            selection,
            limit,
            sort,
            offline,
            refresh,
            timeout,
        }) = cli.command
        else {
            panic!("not a search");
        };
        assert_eq!(query, ["sea surface temperature"]);
        assert_eq!(selection.only, ["zenodo", "datacite"]);
        assert_eq!(
            (limit, sort, offline, refresh),
            (5, Sort::Newest, true, false)
        );
        assert_eq!(timeout, 20);
    }

    #[test]
    fn values_that_look_like_flags_stay_values() {
        let parse = |call: Value| {
            let line = argv("search", &arguments(call)).unwrap();
            Cli::try_parse_from(std::iter::once("dsk".to_owned()).chain(line))
        };
        let Some(Cmd::Search { query, refresh, .. }) =
            parse(json!({ "query": "--refresh" })).unwrap().command
        else {
            panic!("not a search");
        };
        assert_eq!((query, refresh), (vec!["--refresh".to_owned()], false));
        let sort = parse(json!({ "query": "x", "sort": "--offline" }));
        assert_eq!(
            sort.err().unwrap().kind(),
            clap::error::ErrorKind::InvalidValue
        );
    }

    #[test]
    fn unknown_or_mistyped_arguments_are_named() {
        let unknown = argv("sources", &arguments(json!({ "verbose": true })));
        assert!(unknown.unwrap_err().to_string().contains("`verbose`"));
        let mistyped = argv(
            "search",
            &arguments(json!({ "query": "x", "limit": "five" })),
        );
        assert!(mistyped.unwrap_err().to_string().contains("an integer"));
        let single = argv(
            "search",
            &arguments(json!({ "query": "x", "source": "zenodo" })),
        );
        assert!(single.unwrap_err().to_string().contains("a list of strings"));
    }

    #[test]
    fn every_search_has_a_deadline() {
        let given =
            argv("search", &arguments(json!({ "query": "x", "timeout": 5 })))
                .unwrap();
        assert!(given.contains(&"--timeout=5".to_owned()));
        let refused =
            argv("search", &arguments(json!({ "query": "x", "timeout": 0 })));
        assert!(refused.unwrap_err().to_string().contains("deadline"));
    }

    #[test]
    fn a_failure_reads_as_the_command_line_prints_it() {
        let failure = Failure::of(
            &anyhow::Error::from(Error::NoDeadline).context("outer"),
        );
        let result = failed(&failure);
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(
            text.starts_with("Error: outer\n  Cause: timeout 0"),
            "{text}"
        );
        assert_eq!(result["structuredContent"]["event"], "error");
    }
}
