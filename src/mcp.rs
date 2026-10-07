//! `mcp`: a Model Context Protocol server on stdin and stdout, so a chat
//! client without a shell can search (ADR 0015).
//!
//! It speaks the stateless 2026-07-28 revision, where every request carries
//! its protocol version in `_meta`, and the handshake revisions before it for
//! clients that open with `initialize`. One result shape serves both: the
//! older ones allow the extra keys the newer one requires. Each line in is
//! one JSON-RPC message, each line out one response; nothing else ever
//! reaches stdout. A tool call runs as a child process (`tools.rs`) on a
//! thread of its own, so the loop keeps reading and a cancellation can kill
//! it.

mod tools;

use std::collections::HashMap;
use std::io::{self, BufRead, Stdout, Write};
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread::Scope;

use anstream::AutoStream;
use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::output::Out;
use crate::sources::SOURCES;
use crate::{Failure, ui};

pub use tools::Globals;

/// The stateless revision, served to requests that name it in `_meta`.
const CURRENT: &str = "2026-07-28";
/// The handshake revisions, newest first, chosen by `initialize`.
const HANDSHAKE: [&str; 2] = [NEWEST_HANDSHAKE, "2025-06-18"];
const NEWEST_HANDSHAKE: &str = "2025-11-25";
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const CAPABILITIES_KEY: &str = "io.modelcontextprotocol/clientCapabilities";
/// The tool list changes only with the binary, which restarts the server.
const LIST_TTL_MS: u64 = 3_600_000;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const UNSUPPORTED_VERSION: i64 = -32022;

/// What one line from the client asks for.
#[derive(Debug)]
enum Reply {
    /// Answer at once.
    Now(Value),
    /// Run `argv` as a child and answer request `id` when it exits.
    Call {
        id: Value,
        argv: Vec<String>,
    },
    /// Stop the call with this request id; it gets no answer.
    Cancel(Value),
    Nothing,
}

pub fn run(globals: &Globals, out: &Out) -> Result<()> {
    // Resolved once: after an upgrade replaces the binary, the running one's
    // own path can name a deleted file.
    let program = std::env::current_exe().map_err(tools::Error::NoProgram)?;
    let server = Server {
        stdout: Mutex::new(out.stdout()),
        calls: Mutex::new(HashMap::new()),
        serials: AtomicU64::new(0),
        program,
        globals,
    };
    ui::stage(format!(
        "serving {} over MCP on stdio",
        tools::TOOLS.join(", ")
    ));
    std::thread::scope(|scope| {
        let session = server.serve(scope);
        // stdin closed, the client is shutting the server down, or the
        // connection broke; either way no call can be answered any more.
        server.stop_all();
        Ok(session?)
    })
}

struct Server<'a> {
    stdout: Mutex<AutoStream<Stdout>>,
    /// Calls in flight, by request id as JSON text.
    calls: Mutex<HashMap<String, Call>>,
    /// The serial the next call gets.
    serials: AtomicU64,
    /// This binary, which every call runs.
    program: PathBuf,
    globals: &'a Globals,
}

impl Server<'_> {
    /// Answer each line on stdin, until it closes or a response cannot be
    /// written.
    fn serve<'scope>(
        &'scope self,
        scope: &'scope Scope<'scope, '_>,
    ) -> io::Result<()> {
        let mut stdin = io::stdin().lock();
        let mut line = Vec::new();
        loop {
            line.clear();
            if stdin.read_until(b'\n', &mut line)? == 0 {
                return Ok(());
            }
            match handle(&line) {
                Reply::Now(response) => self.send(&response)?,
                Reply::Call { id, argv } => match self.start(&id, &argv) {
                    Ok(serial) => {
                        scope.spawn(move || self.finish(&id, serial));
                    }
                    Err(refusal) => self.send(&refusal)?,
                },
                Reply::Cancel(id) => self.stop(&id),
                Reply::Nothing => {}
            }
        }
    }

    fn send(&self, message: &Value) -> io::Result<()> {
        let mut w = self.stdout.lock().unwrap_or_else(PoisonError::into_inner);
        writeln!(w, "{message}")?;
        w.flush()
    }

    /// Spawn the call's child and record it under a new serial, or say why
    /// it could not start.
    fn start(&self, id: &Value, argv: &[String]) -> Result<u64, Value> {
        let mut calls =
            self.calls.lock().unwrap_or_else(PoisonError::into_inner);
        let key = id.to_string();
        if calls.contains_key(&key) {
            return Err(error(
                id,
                INVALID_REQUEST,
                "a request with this id is still running",
                None,
            ));
        }
        if calls.len() >= tools::MAX_CALLS {
            let busy = anyhow::Error::from(tools::Error::Busy);
            return Err(complete(id, tools::failed(&Failure::of(&busy))));
        }
        match tools::spawn(&self.program, argv, self.globals) {
            Ok(child) => {
                let serial = self.serials.fetch_add(1, Ordering::Relaxed);
                calls.insert(key, Call { serial, child });
                Ok(serial)
            }
            Err(e) => Err(error(
                id,
                INTERNAL_ERROR,
                &format!("cannot start {}: {e}", crate::invoked_name()),
                None,
            )),
        }
    }

    /// Wait for the call's child, then answer, unless it was stopped. The
    /// serial keeps a stopped call's thread off a later call with its id.
    fn finish(&self, id: &Value, serial: u64) {
        let key = id.to_string();
        let pipes = {
            let mut calls =
                self.calls.lock().unwrap_or_else(PoisonError::into_inner);
            calls.get_mut(&key).filter(|call| call.serial == serial).map(
                |call| (call.child.stdout.take(), call.child.stderr.take()),
            )
        };
        let Some((stdout, stderr)) = pipes else { return };
        let output = tools::collect(stdout, stderr);
        let Some(mut child) = self.take(&key, serial) else { return };
        let response = match (child.wait(), output) {
            (Ok(status), Ok(output)) => tools::result(status, &output),
            (Err(e), _) => Err(format!("cannot wait for the command: {e}")),
            (_, Err(e)) => {
                Err(format!("cannot read what the command printed: {e}"))
            }
        };
        let response = response.map_or_else(
            |why| error(id, INTERNAL_ERROR, &why, None),
            |result| complete(id, result),
        );
        if let Err(e) = self.send(&response) {
            ui::warn(format!("cannot answer request {key}: {e}"));
        }
    }

    /// The child of the call recorded under `key`, removed, if it is still
    /// the call with this serial.
    fn take(&self, key: &str, serial: u64) -> Option<Child> {
        let mut calls =
            self.calls.lock().unwrap_or_else(PoisonError::into_inner);
        if calls.get(key)?.serial != serial {
            return None;
        }
        calls.remove(key).map(|call| call.child)
    }

    fn stop(&self, id: &Value) {
        let call = self
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id.to_string());
        call.into_iter().for_each(end);
    }

    fn stop_all(&self) {
        let calls: Vec<Call> = self
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(_, call)| call)
            .collect();
        calls.into_iter().for_each(end);
    }
}

/// A call in flight: its child, and a serial that tells it from a later
/// call reusing its request id.
struct Call {
    serial: u64,
    child: Child,
}

/// Kill a call's child and reap it. Its thread then reads the closed pipes
/// and, finding the call gone, answers nothing.
fn end(mut call: Call) {
    let _ = call.child.kill();
    let _ = call.child.wait();
}

/// Decide what one line asks for. Pure, so every protocol rule is a unit
/// test.
fn handle(line: &[u8]) -> Reply {
    if line.trim_ascii().is_empty() {
        return Reply::Nothing;
    }
    let Ok(message) = serde_json::from_slice::<Value>(line) else {
        return Reply::Now(error(
            &Value::Null,
            PARSE_ERROR,
            "not JSON in UTF-8",
            None,
        ));
    };
    let Some(fields) = message.as_object() else {
        return Reply::Now(error(
            &Value::Null,
            INVALID_REQUEST,
            "a message is one JSON object; batches are not part of MCP",
            None,
        ));
    };
    let version_ok = fields.get("jsonrpc") == Some(&json!("2.0"));
    let method = fields.get("method").and_then(Value::as_str);
    let empty = Map::new();
    let params =
        fields.get("params").and_then(Value::as_object).unwrap_or(&empty);
    // A notification is never answered, not even to say it is malformed.
    let Some(id) = fields.get("id") else {
        return match method {
            Some("notifications/cancelled") if version_ok => params
                .get("requestId")
                .cloned()
                .map_or(Reply::Nothing, Reply::Cancel),
            _ => Reply::Nothing,
        };
    };
    // A response to a request this server never sends.
    if fields.contains_key("result") || fields.contains_key("error") {
        return Reply::Nothing;
    }
    let (Some(method), true) = (method, version_ok) else {
        let reason = if version_ok {
            "a request names its method as a string"
        } else {
            "jsonrpc must be \"2.0\""
        };
        return Reply::Now(error(id, INVALID_REQUEST, reason, None));
    };
    if !(id.is_string() || id.is_number()) {
        return Reply::Now(error(
            &Value::Null,
            INVALID_REQUEST,
            "a request id is a string or a number",
            None,
        ));
    }
    respond(id, method, params)
}

/// Answer a well-formed request.
fn respond(id: &Value, method: &str, params: &Map<String, Value>) -> Reply {
    if let Some(refusal) = check_meta(id, params) {
        return Reply::Now(refusal);
    }
    match method {
        "server/discover" => Reply::Now(complete(id, discover())),
        "initialize" => Reply::Now(complete(id, initialize(params))),
        "ping" => Reply::Now(complete(id, json!({}))),
        "tools/list" => Reply::Now(match tools::definitions() {
            Ok(definitions) => complete(
                id,
                json!({
                    "tools": definitions,
                    "ttlMs": LIST_TTL_MS,
                    "cacheScope": "public",
                }),
            ),
            Err(e) => error(id, INTERNAL_ERROR, &e.to_string(), None),
        }),
        "tools/call" => call(id, params),
        other => Reply::Now(error(
            id,
            METHOD_NOT_FOUND,
            &format!("unknown method {other}"),
            None,
        )),
    }
}

/// A request that names a protocol version in `_meta` is served under it,
/// so the version must be one this server speaks and the request must carry
/// the client's capabilities, which that revision requires.
fn check_meta(id: &Value, params: &Map<String, Value>) -> Option<Value> {
    let meta = params.get("_meta")?;
    let version = meta.get(VERSION_KEY)?;
    if version != CURRENT {
        return Some(error(
            id,
            UNSUPPORTED_VERSION,
            "Unsupported protocol version",
            Some(json!({ "supported": supported(), "requested": version })),
        ));
    }
    if meta.get(CAPABILITIES_KEY).is_none() {
        return Some(error(
            id,
            INVALID_PARAMS,
            &format!(
                "a {CURRENT} request carries {CAPABILITIES_KEY} in _meta"
            ),
            None,
        ));
    }
    None
}

fn call(id: &Value, params: &Map<String, Value>) -> Reply {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Reply::Now(error(
            id,
            INVALID_PARAMS,
            "tools/call names a tool",
            None,
        ));
    };
    let Some(tool) = tools::find(name) else {
        return Reply::Now(error(
            id,
            INVALID_PARAMS,
            &format!("Unknown tool: {name}"),
            None,
        ));
    };
    let empty = Map::new();
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(arguments)) => arguments,
        Some(_) => {
            return Reply::Now(error(
                id,
                INVALID_PARAMS,
                "tool arguments are a JSON object",
                None,
            ));
        }
    };
    match tools::argv(tool, arguments) {
        Ok(argv) => Reply::Call { id: id.clone(), argv },
        Err(e) => Reply::Now(complete(id, tools::failed(&Failure::of(&e)))),
    }
}

fn supported() -> Vec<&'static str> {
    std::iter::once(CURRENT).chain(HANDSHAKE).collect()
}

fn server_info() -> Value {
    json!({ "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION") })
}

fn instructions() -> String {
    format!(
        "dataseek searches {} dataset sources with one query. `search` \
        returns one merged, deduplicated, ranked list; `sources` lists the \
        sources and whether their keys are set; `inspect` reads one dataset \
        page's schema.org or Croissant metadata.",
        SOURCES.len()
    )
}

fn discover() -> Value {
    json!({
        "supportedVersions": supported(),
        "capabilities": { "tools": {} },
        "instructions": instructions(),
        "ttlMs": LIST_TTL_MS,
        "cacheScope": "public",
    })
}

/// The handshake: the client's version when this server speaks it, else
/// the newest handshake revision, for the client to accept or leave.
fn initialize(params: &Map<String, Value>) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = HANDSHAKE
        .into_iter()
        .find(|v| Some(*v) == asked)
        .unwrap_or(NEWEST_HANDSHAKE);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": server_info(),
        "instructions": instructions(),
    })
}

/// A successful response, marked as final and signed with the server's name
/// as the stateless revision asks.
fn complete(id: &Value, mut result: Value) -> Value {
    if let Some(fields) = result.as_object_mut() {
        fields.insert("resultType".into(), json!("complete"));
        fields.insert(
            "_meta".into(),
            json!({ "io.modelcontextprotocol/serverInfo": server_info() }),
        );
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let (Some(fields), Some(data)) = (error.as_object_mut(), data) {
        fields.insert("data".into(), data);
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now(line: &str) -> Value {
        match handle(line.as_bytes()) {
            Reply::Now(response) => response,
            other => panic!("expected an answer, got {other:?}"),
        }
    }

    fn request(method: &str, params: &Value) -> String {
        json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params })
            .to_string()
    }

    fn current() -> Value {
        json!({ "_meta": { VERSION_KEY: CURRENT, CAPABILITIES_KEY: {} } })
    }

    // A call stopped and replaced by a newer one with the same id: the
    // stopped call's thread must leave the newer call running.
    #[cfg(unix)]
    #[test]
    fn a_stopped_call_never_collects_a_newer_call_with_its_id() {
        let globals = Globals {
            quiet: false,
            verbose: 0,
            cache_dir: None,
            connect_timeout: 1,
        };
        let server = Server {
            stdout: Mutex::new(AutoStream::never(io::stdout())),
            calls: Mutex::new(HashMap::new()),
            serials: AtomicU64::new(0),
            program: PathBuf::from("/bin/sh"),
            globals: &globals,
        };
        let id = json!(1);
        let sleep = ["-c".to_owned(), "exec sleep 10".to_owned()];
        let stopped = server.start(&id, &sleep).unwrap();
        server.stop(&id);
        let newer = server.start(&id, &sleep).unwrap();
        let started = std::time::Instant::now();
        server.finish(&id, stopped);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        let serial = server.calls.lock().unwrap().get("1").map(|c| c.serial);
        assert_eq!(serial, Some(newer));
        server.stop_all();
    }

    #[test]
    fn malformed_messages_get_the_json_rpc_codes() {
        assert_eq!(now("{not json")["error"]["code"], PARSE_ERROR);
        assert!(matches!(
            handle(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\"}"),
            Reply::Now(response) if response["error"]["code"] == PARSE_ERROR
        ));
        for nameless in [
            r#"{"jsonrpc":"2.0","id":4}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":5}"#,
        ] {
            let refused = now(nameless);
            assert_eq!(
                refused["error"]["code"], INVALID_REQUEST,
                "{nameless}"
            );
            assert_eq!(refused["id"], 4, "{nameless}");
        }
        assert_eq!(now("[1, 2]")["error"]["code"], INVALID_REQUEST);
        assert_eq!(
            now(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#)["error"]["code"],
            INVALID_REQUEST
        );
        assert_eq!(
            now(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#)["error"]["code"],
            INVALID_REQUEST
        );
        let unknown = now(&request("resources/list", &json!({})));
        assert_eq!(unknown["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(unknown["id"], 7);
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        let silent = [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":3,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":3,"error":{"code":1,"message":"no"}}"#,
            r#"{"jsonrpc":"1.0","method":"notifications/initialized"}"#,
            r#"{"method":"notifications/cancelled","params":{"requestId":"a"}}"#,
        ];
        for line in silent {
            assert!(
                matches!(handle(line.as_bytes()), Reply::Nothing),
                "{line}"
            );
        }
        let cancel = handle(
            br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"a"}}"#,
        );
        assert!(matches!(cancel, Reply::Cancel(id) if id == "a"));
    }

    #[test]
    fn initialize_keeps_a_known_version_and_offers_the_newest_otherwise() {
        let known = now(&request(
            "initialize",
            &json!({"protocolVersion": "2025-06-18"}),
        ));
        assert_eq!(known["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(known["result"]["serverInfo"]["name"], "dataseek");
        assert_eq!(known["result"]["capabilities"], json!({"tools": {}}));
        let old = now(&request(
            "initialize",
            &json!({"protocolVersion": "2024-11-05"}),
        ));
        assert_eq!(old["result"]["protocolVersion"], NEWEST_HANDSHAKE);
    }

    #[test]
    fn current_requests_are_checked_against_their_meta() {
        let discover = now(&request("server/discover", &current()));
        assert_eq!(discover["result"]["resultType"], "complete");
        assert!(
            discover["result"]["supportedVersions"]
                .as_array()
                .unwrap()
                .contains(&json!(CURRENT))
        );
        assert_eq!(
            discover["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]
                ["name"],
            "dataseek"
        );

        let future = now(&request(
            "tools/list",
            &json!({"_meta": {VERSION_KEY: "1900-01-01", CAPABILITIES_KEY: {}}}),
        ));
        assert_eq!(future["error"]["code"], UNSUPPORTED_VERSION);
        assert_eq!(future["error"]["data"]["requested"], "1900-01-01");

        let bare = now(&request(
            "tools/list",
            &json!({"_meta": {VERSION_KEY: CURRENT}}),
        ));
        assert_eq!(bare["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn the_tool_list_is_cacheable_and_in_a_fixed_order() {
        let list = now(&request("tools/list", &current()));
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, tools::TOOLS);
        assert_eq!(list["result"]["ttlMs"], LIST_TTL_MS);
        assert_eq!(list["result"]["cacheScope"], "public");
    }

    #[test]
    fn calls_are_checked_before_anything_runs() {
        let unknown = now(&request("tools/call", &json!({"name": "delete"})));
        assert_eq!(unknown["error"]["code"], INVALID_PARAMS);
        let shapeless = now(&request(
            "tools/call",
            &json!({"name": "search", "arguments": [1]}),
        ));
        assert_eq!(shapeless["error"]["code"], INVALID_PARAMS);
        // A bad argument is the model's to fix, so it is a tool error.
        let refused = now(&request(
            "tools/call",
            &json!({"name": "search", "arguments": {"query": "x", "timeout": 0}}),
        ));
        assert_eq!(refused["result"]["isError"], true);
        assert!(matches!(
            handle(request("tools/call", &json!({"name": "sources"})).as_bytes()),
            Reply::Call { argv, .. } if argv == ["sources", "--json"]
        ));
    }
}
