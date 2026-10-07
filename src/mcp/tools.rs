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

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::ui::EVENTS_SCHEMA;
use crate::{Failure, help, ui};

/// The tools, in the order `tools/list` returns them.
pub const TOOLS: [&str; 3] = ["search", "sources", "inspect"];

/// What to try after a bad argument: the command line's hint, to run
/// `--help`, is no use to a model.
const USAGE_HINT: &str =
    "pass the arguments tools/list gives this tool, with the values it lists";

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
    #[error("{tool} needs `{key}`\n  Try:   {USAGE_HINT}")]
    Missing { tool: &'static str, key: String },
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
    if let Some(missing) =
        flags.iter().find(|a| a.required && !arguments.contains_key(a.key()))
    {
        return Err(
            Error::Missing { tool, key: missing.key().to_owned() }.into()
        );
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

/// How many of a child's last stderr lines that are not events an internal
/// error quotes.
const STRAY_LINES: usize = 5;

/// What a child left: its whole stdout, the failure its error event
/// describes, and its last stderr lines that were not events, such as a
/// panic message. Every stderr line has already been relayed to ours.
pub struct Output {
    stdout: Vec<u8>,
    failure: Option<Failure>,
    stray: VecDeque<String>,
}

/// Read a child's two pipes to their ends, relaying its stderr as it comes.
pub fn collect(
    stdout: Option<impl Read>,
    stderr: Option<impl Read + Send>,
) -> io::Result<Output> {
    std::thread::scope(|scope| {
        let relay = scope.spawn(|| relay(stderr));
        let mut bytes = Vec::new();
        let read = stdout.map_or(Ok(0), |mut s| s.read_to_end(&mut bytes));
        let (failure, stray) = relay.join().map_err(|_| {
            io::Error::other("the thread relaying its stderr panicked")
        })??;
        read?;
        Ok(Output { stdout: bytes, failure, stray })
    })
}

/// Pass each of a child's stderr lines on to ours, keeping the failure its
/// error event describes and the last lines that were not events. Lines are
/// read as bytes, so text that is not UTF-8 cannot stop the reading and
/// leave the child blocked on a full pipe.
fn relay(
    stderr: Option<impl Read>,
) -> io::Result<(Option<Failure>, VecDeque<String>)> {
    let mut failure = None;
    let mut stray = VecDeque::new();
    let Some(stderr) = stderr else {
        return Ok((failure, stray));
    };
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? > 0 {
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end();
        ui::relay(text);
        match serde_json::from_str::<Value>(text) {
            Ok(event)
                if event.get("schema") == Some(&json!(EVENTS_SCHEMA)) =>
            {
                failure = failure_of(&event).or(failure);
            }
            _ if text.is_empty() => {}
            _ => {
                if stray.len() == STRAY_LINES {
                    stray.pop_front();
                }
                stray.push_back(text.to_owned());
            }
        }
        line.clear();
    }
    Ok((failure, stray))
}

/// The failure an error event describes, with a hint a model can follow in
/// place of the command line's.
fn failure_of(event: &Value) -> Option<Failure> {
    if event.get("event") != Some(&json!("error")) {
        return None;
    }
    let mut failure = Failure::deserialize(event).ok()?;
    if failure.hint == crate::USAGE_HINT {
        USAGE_HINT.clone_into(&mut failure.hint);
    }
    Some(failure)
}

/// The `tools/call` result for a child that exited with `status`: the JSON
/// object it printed, or its error event as a failed call. A run that
/// succeeded without printing a JSON object, or failed without an error
/// event, is a fault of the server's, and the `Err` says what it saw.
pub fn result(status: ExitStatus, output: &Output) -> Result<Value, String> {
    if status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        let text = text.trim_end();
        return match serde_json::from_str(text) {
            Ok(Value::Object(object)) => Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": object,
                "isError": false,
            })),
            Ok(_) => {
                Err("the command printed JSON that is not an object".into())
            }
            Err(e) => Err(format!("the command printed no JSON object: {e}")),
        };
    }
    if let Some(failure) = &output.failure {
        return Ok(failed(failure));
    }
    let mut why =
        format!("the command exited with {status} and no error event");
    if !output.stray.is_empty() {
        why.push_str("; its stderr ended with:");
        for line in &output.stray {
            why.push('\n');
            why.push_str(line);
        }
    }
    Err(why)
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
        let missing = argv("search", &arguments(json!({ "limit": 5 })));
        let missing = missing.unwrap_err().to_string();
        assert!(missing.starts_with("search needs `query`"), "{missing}");
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

    fn exit(success: bool) -> ExitStatus {
        #[cfg(unix)]
        let status =
            std::os::unix::process::ExitStatusExt::from_raw(if success {
                0
            } else {
                256
            });
        #[cfg(windows)]
        let status = std::os::windows::process::ExitStatusExt::from_raw(
            u32::from(!success),
        );
        status
    }

    fn output(stdout: &[u8], stderr: &[u8]) -> Output {
        collect(Some(stdout), Some(stderr)).unwrap()
    }

    #[test]
    fn a_run_that_left_nothing_usable_says_what_it_left() {
        let prose = result(exit(true), &output(b"Usage: dsk", b""));
        let why = prose.unwrap_err();
        assert!(why.contains("no JSON object: expected value"), "{why}");
        let list = result(exit(true), &output(b"[1]", b""));
        assert!(list.unwrap_err().contains("not an object"));

        let stderr = b"{\"schema\":\"dataseek-events/1\",\"event\":\"stage\",\
            \"message\":\"searching\"}\nthread 'main' panicked at src/x.rs:1:1:\n\
            \xffboom\n";
        let why = result(exit(false), &output(b"", stderr)).unwrap_err();
        assert!(why.contains("no error event"), "{why}");
        assert!(why.contains("\nthread 'main' panicked at src/x.rs:1:1:"));
        assert!(why.ends_with("boom") && !why.contains("searching"), "{why}");
    }

    #[test]
    fn an_error_event_becomes_the_failed_call() {
        let stderr = br#"{"schema":"dataseek-events/1","event":"error","message":"no source","causes":["why"],"try":"this"}"#;
        let call = result(exit(false), &output(b"", stderr)).unwrap();
        assert_eq!(call["isError"], true);
        let text = call["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, "Error: no source\n  Cause: why\n  Try:   this");
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
