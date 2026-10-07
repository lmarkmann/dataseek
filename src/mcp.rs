//! `mcp`: a Model Context Protocol server on stdin and stdout, so a chat
//! client without a shell can search (ADR 0014).
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
use std::process::Child;
use std::sync::{Mutex, PoisonError};

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
    let server = Server {
        stdout: Mutex::new(out.stdout()),
        calls: Mutex::new(HashMap::new()),
        globals,
    };
    ui::stage(format!(
        "serving {} over MCP on stdio",
        tools::TOOLS.join(", ")
    ));
    std::thread::scope(|scope| {
        let session = (|| -> io::Result<()> {
            for line in io::stdin().lock().lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                match handle(&line) {
                    Reply::Now(response) => server.send(&response)?,
                    Reply::Call { id, argv } => {
                        if let Some(refusal) = server.start(&id, &argv) {
                            server.send(&refusal)?;
                        } else {
                            let server = &server;
                            scope.spawn(move || server.finish(&id));
                        }
                    }
                    Reply::Cancel(id) => server.stop(&id),
                    Reply::Nothing => {}
                }
            }
            Ok(())
        })();
        // stdin closed, the client is shutting the server down, or the
        // connection broke; either way no call can be answered any more.
        server.stop_all();
        Ok(session?)
    })
}

struct Server<'a> {
    stdout: Mutex<AutoStream<Stdout>>,
    /// Calls in flight, by request id as JSON text.
    calls: Mutex<HashMap<String, Child>>,
    globals: &'a Globals,
}

impl Server<'_> {
    fn send(&self, message: &Value) -> io::Result<()> {
        let mut w = self.stdout.lock().unwrap_or_else(PoisonError::into_inner);
        writeln!(w, "{message}")?;
        w.flush()
    }

    /// Spawn the call's child and record it, or say why it could not start.
    fn start(&self, id: &Value, argv: &[String]) -> Option<Value> {
        let mut calls =
            self.calls.lock().unwrap_or_else(PoisonError::into_inner);
        let key = id.to_string();
        if calls.contains_key(&key) {
            return Some(error(
                id,
                INVALID_REQUEST,
                "a request with this id is still running",
            ));
        }
        match tools::spawn(argv, self.globals) {
            Ok(child) => {
                calls.insert(key, child);
                None
            }
            Err(e) => Some(error(
                id,
                INTERNAL_ERROR,
                &format!("cannot start {}: {e}", crate::invoked_name()),
            )),
        }
    }

    /// Wait for the call's child, then answer, unless it was stopped.
    fn finish(&self, id: &Value) {
        let key = id.to_string();
        let pipes = {
            let mut calls =
                self.calls.lock().unwrap_or_else(PoisonError::into_inner);
            calls
                .get_mut(&key)
                .map(|child| (child.stdout.take(), child.stderr.take()))
        };
        let Some((stdout, stderr)) = pipes else { return };
        let output = tools::collect(stdout, stderr);
        let call = self
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key);
        let Some(mut child) = call else { return };
        let response = match child.wait() {
            Ok(status) => match tools::result(status, &output) {
                Some(result) => complete(id, result),
                None => error(
                    id,
                    INTERNAL_ERROR,
                    &format!(
                        "the command exited with {status} and reported nothing"
                    ),
                ),
            },
            Err(e) => error(id, INTERNAL_ERROR, &e.to_string()),
        };
        if let Err(e) = self.send(&response) {
            ui::warn(format!("cannot answer request {key}: {e}"));
        }
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
        let calls: Vec<Child> = self
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(_, child)| child)
            .collect();
        calls.into_iter().for_each(end);
    }
}

/// Kill a call's child and reap it. Its thread then reads the closed pipes
/// and, finding the call gone, answers nothing.
fn end(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Decide what one line asks for. Pure, so every protocol rule is a unit
/// test.
fn handle(line: &str) -> Reply {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Reply::Now(error(&Value::Null, PARSE_ERROR, "not JSON"));
    };
    let Some(fields) = message.as_object() else {
        return Reply::Now(error(
            &Value::Null,
            INVALID_REQUEST,
            "a message is one JSON object; batches are not part of MCP",
        ));
    };
    let id = fields.get("id");
    if fields.get("jsonrpc") != Some(&json!("2.0")) {
        return Reply::Now(error(
            id.unwrap_or(&Value::Null),
            INVALID_REQUEST,
            "jsonrpc must be \"2.0\"",
        ));
    }
    let Some(method) = fields.get("method").and_then(Value::as_str) else {
        // A response to a request this server never sends.
        return Reply::Nothing;
    };
    let empty = Map::new();
    let params =
        fields.get("params").and_then(Value::as_object).unwrap_or(&empty);
    let Some(id) = id else {
        return match method {
            "notifications/cancelled" => params
                .get("requestId")
                .cloned()
                .map_or(Reply::Nothing, Reply::Cancel),
            _ => Reply::Nothing,
        };
    };
    if !(id.is_string() || id.is_number()) {
        return Reply::Now(error(
            &Value::Null,
            INVALID_REQUEST,
            "a request id is a string or a number",
        ));
    }
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
            Err(e) => error(id, INTERNAL_ERROR, &e.to_string()),
        }),
        "tools/call" => call(id, params),
        other => Reply::Now(error(
            id,
            METHOD_NOT_FOUND,
            &format!("unknown method {other}"),
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
        return Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": UNSUPPORTED_VERSION,
                "message": "Unsupported protocol version",
                "data": { "supported": supported(), "requested": version },
            },
        }));
    }
    if meta.get(CAPABILITIES_KEY).is_none() {
        return Some(error(
            id,
            INVALID_PARAMS,
            &format!(
                "a {CURRENT} request carries {CAPABILITIES_KEY} in _meta"
            ),
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
        ));
    };
    let Some(tool) = tools::find(name) else {
        return Reply::Now(error(
            id,
            INVALID_PARAMS,
            &format!("Unknown tool: {name}"),
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

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now(line: &str) -> Value {
        match handle(line) {
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

    #[test]
    fn malformed_messages_get_the_json_rpc_codes() {
        assert_eq!(now("{not json")["error"]["code"], PARSE_ERROR);
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
        assert!(matches!(
            handle(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            ),
            Reply::Nothing
        ));
        assert!(matches!(
            handle(r#"{"jsonrpc":"2.0","id":3,"result":{}}"#),
            Reply::Nothing
        ));
        let cancel = handle(
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"a"}}"#,
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
            handle(&request("tools/call", &json!({"name": "sources"}))),
            Reply::Call { argv, .. } if argv == ["sources", "--json"]
        ));
    }
}
