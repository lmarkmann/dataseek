//! Tests of the CLI surface itself: flags, exit codes, pipe behavior, and the
//! shape of stdout. These run the built binary, not the library functions.
//!
//! Every run gets its own home, config and cache directories, so a key or a
//! cache on the developer's machine can never change an outcome, and no test
//! reaches the network: searches go through a proxy on a closed port, which
//! is how an offline machine looks to dataseek.

// A panic in a helper here is the assertion failing, which is the point.
// `clippy.toml` exempts `#[test]` bodies; these helpers sit outside one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "cli/filters.rs"]
mod filters;

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};

const KEY_VARS: [&str; 8] = [
    "HF_TOKEN",
    "KAGGLE_API_TOKEN",
    "DATAGOV_API_KEY",
    "GITHUB_TOKEN",
    "FRED_API_KEY",
    "ROBOFLOW_API_KEY",
    "DATACOMMONS_API_KEY",
    "NCBI_API_KEY",
];

const OWN_VARS: [&str; 6] = [
    "DATASEEK_EXCLUDE",
    "DATASEEK_PER_SOURCE",
    "DATASEEK_LIMIT",
    "DATASEEK_TIMEOUT",
    "DATASEEK_CACHE_DIR",
    "DATASEEK_CONNECT_TIMEOUT",
];

struct Sandbox {
    dir: tempfile::TempDir,
    cmd: Command,
}

impl Sandbox {
    fn cache(&self) -> PathBuf {
        self.dir.path().join("cache").join("dataseek")
    }
}

impl std::ops::Deref for Sandbox {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.cmd
    }
}

impl std::ops::DerefMut for Sandbox {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.cmd
    }
}

/// `program` with the sandbox environment: its own directories, no keys, no
/// dataseek settings, and every request sent to a proxy on a closed port.
fn sandboxed(program: &Path, dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    // wrap_help reads COLUMNS; pin it so the snapshot does not depend on the
    // runner's terminal.
    cmd.env("COLUMNS", "100")
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("KAGGLE_CONFIG_DIR", dir.join("kaggle"))
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9");
    // ureq prefers ALL_PROXY and honors NO_PROXY, so either would let a
    // request past the closed port.
    let proxy_vars = [
        "ALL_PROXY",
        "all_proxy",
        "https_proxy",
        "http_proxy",
        "NO_PROXY",
        "no_proxy",
    ];
    for var in KEY_VARS.iter().chain(&OWN_VARS).chain(&proxy_vars) {
        cmd.env_remove(var);
    }
    cmd
}

fn bin_named(name: &str) -> Sandbox {
    let dir = tempfile::tempdir().unwrap();
    let program = assert_cmd::cargo::cargo_bin(name);
    let cmd = Command::from_std(sandboxed(&program, dir.path()));
    Sandbox { dir, cmd }
}

fn bin() -> Sandbox {
    bin_named("dataseek")
}

fn stdout_text(args: &[&str]) -> String {
    let out = bin().args(args).output().unwrap();
    assert!(out.status.success(), "{args:?} failed: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn json_of(args: &[&str]) -> Value {
    serde_json::from_str(&stdout_text(args)).unwrap()
}

fn result_titles(stdout: &[u8]) -> Vec<String> {
    let report: Value = serde_json::from_slice(stdout).unwrap();
    report
        .get("results")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|r| r.get("title").and_then(Value::as_str).unwrap().to_owned())
        .collect()
}

#[test]
fn version_matches_manifest() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("dataseek {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_exits_zero_and_lists_commands() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("sources"))
        .stdout(predicate::str::contains("bench"));
}

// The bare invocation is orientation, so it is data: on stdout, exit 0, small
// enough for one screen, and pointing at the full help.
#[test]
fn naked_invocation_prints_the_overview_on_stdout() {
    let out = bin().output().unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with(&format!(
        "\u{25c6} dataseek v{}",
        env!("CARGO_PKG_VERSION")
    )));
    for group in ["search\n", "upkeep\n", "shell\n"] {
        assert!(text.contains(group), "{group} missing:\n{text}");
    }
    assert!(text.ends_with("run dataseek -h for full usage\n"), "{text}");
    assert!(text.lines().count() <= 24, "{text}");
}

#[test]
fn the_overview_names_the_program_that_ran() {
    let out = bin_named("dsk").arg("--no-color").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.ends_with("run dsk -h for full usage\n"), "{text}");
}

#[test]
fn plain_overview_is_ascii() {
    let text = stdout_text(&["--plain"]);
    assert!(text.is_ascii(), "{text}");
}

// The examples are capped at three on the root; the long help adds the exit
// codes, the topics and where to report a bug.
#[test]
fn short_help_has_three_examples_and_long_help_the_rest() {
    let short = stdout_text(&["-h"]);
    let examples = short.split("Examples:\n").nth(1).unwrap();
    assert_eq!(examples.lines().count(), 3, "{examples}");
    let long = stdout_text(&["--help"]);
    assert!(long.contains("Exit status: 0 success"));
    assert!(long.contains(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues")));
}

#[test]
fn completion_emits_a_script_for_every_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let out = bin().args(["completion", shell]).output().unwrap();
        assert!(out.status.success(), "{shell} failed");
        assert!(!out.stdout.is_empty(), "{shell} produced nothing");
        assert!(out.stderr.is_empty(), "{shell} wrote to stderr");
        assert!(!out.stdout.contains(&0x1b), "{shell} leaked ANSI");
        let script = String::from_utf8_lossy(&out.stdout);
        assert!(
            script.contains(env!("CARGO_PKG_NAME")),
            "{shell} script does not name the binary"
        );
    }
}

#[test]
fn completions_offer_source_ids() {
    let script = stdout_text(&["completion", "fish"]);
    assert!(
        script.contains("huggingface"),
        "source ids missing:\n{script:.400}"
    );
}

/// `program` ready to run when it is installed. On CI a missing tool fails
/// the test; locally the check is skipped.
fn tool(program: &str) -> Option<std::process::Command> {
    let found = std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success());
    if found {
        return Some(std::process::Command::new(program));
    }
    assert!(std::env::var_os("CI").is_none(), "{program} is not installed");
    None
}

#[test]
fn the_fish_completion_script_parses() {
    let Some(mut fish) = tool("fish") else { return };
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("dataseek.fish");
    std::fs::write(&script, stdout_text(&["completion", "fish"])).unwrap();
    let out = fish.arg("--no-execute").arg(&script).output().unwrap();
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn a_bad_flag_value_exits_two() {
    bin()
        .args(["sources", "--color=chartreuse"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
}

#[test]
fn an_unknown_source_id_is_a_usage_error() {
    bin()
        .args(["search", "climate", "--source", "not-a-source"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn search_needs_a_query() {
    bin().arg("search").assert().code(2);
}

#[test]
fn quiet_conflicts_with_verbose() {
    bin().args(["sources", "-q", "-v"]).assert().code(2);
}

#[test]
fn settings_are_read_from_the_environment() {
    let mut cmd = bin();
    cmd.env("DATASEEK_LIMIT", "lots");
    cmd.args(["search", "climate"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("lots"));
    let help = stdout_text(&["search", "--help"]);
    assert!(help.contains("[env: DATASEEK_LIMIT="), "{help}");
}

#[test]
fn offline_search_fails_with_a_hint_and_clean_stdout() {
    let out = bin()
        .args(["search", "climate", "-s", "datacite,zenodo"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "an offline search must exit 1");
    assert!(out.stdout.is_empty(), "an error put bytes on stdout");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let error = stderr.find("Error:").expect("no Error: line");
    let hint = stderr.find("  Try:").expect("no Try: line");
    assert!(error < hint, "wrong order:\n{stderr}");
    assert!(stderr.contains("looks offline"), "{stderr}");
}

// A piped stderr gets the status lines a terminal would, as plain lines: no
// carriage returns, no escapes, and the start announced before the error.
#[test]
fn a_piped_search_announces_itself_without_frames() {
    let out = bin()
        .args(["search", "climate", "-s", "datacite,zenodo"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let start = stderr.find("searching 2 sources").expect(&stderr);
    assert!(start < stderr.find("Error:").unwrap(), "{stderr}");
    assert!(!stderr.contains('\r') && !stderr.contains('\u{1b}'), "{stderr}");
}

#[test]
fn quiet_keeps_only_the_error() {
    let out = bin()
        .args(["search", "climate", "-s", "datacite", "-q"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("Error:"), "{stderr}");
}

#[test]
fn errors_under_json_are_events_on_stderr() {
    let out = bin()
        .args(["search", "climate", "-s", "roboflow", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "an error put bytes on stdout");
    let stderr = String::from_utf8(out.stderr).unwrap();
    let events: Vec<Value> =
        stderr.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let error = events.last().unwrap();
    assert_eq!(error["schema"], "dataseek-events/1");
    assert_eq!(error["event"], "error");
    assert!(error["try"].as_str().unwrap().contains("dataseek sources"));
    filters::with_snapshot_filters(|| {
        insta::assert_snapshot!("json_error_event", stderr);
    });

    // -v adds per-source notes, as events too.
    let out = bin()
        .args(["search", "climate", "-s", "datacite", "--json", "-v"])
        .output()
        .unwrap();
    let events: Vec<Value> = String::from_utf8(out.stderr)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(events.iter().any(|e| e["event"] == "note"), "{events:?}");

    // clap's own usage errors take the same shape, with clap's details as
    // causes.
    let out = bin().args(["--json", "no-such-command"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let event: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(event["event"], "error");
    let out = bin()
        .args(["--json", "search", "x", "--sort", "oldest"])
        .output()
        .unwrap();
    let event: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(
        event["message"],
        "invalid value 'oldest' for '--sort <ORDER>'"
    );
    assert_eq!(
        event["causes"],
        json!(["[possible values: relevance, newest]"])
    );
    let out = bin().args(["--json", "search"]).output().unwrap();
    let event: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(event["causes"], json!(["<QUERY>..."]));
}

// A script written for --jq must fail as a usage error that names the
// replacement, never run a search with "--jq" swallowed into the query.
#[test]
fn the_removed_jq_flag_points_at_json_and_jq() {
    let out = bin()
        .args(["search", "census", "--jq", ".results[].url", "--offline"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out.stdout, b"");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("pipe --json into jq"), "{stderr}");
    assert!(!stderr.contains("-- --jq"), "{stderr}");

    let out = bin()
        .args(["--json", "sources", "--jq", ".sources"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let event: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(event["event"], "error");
}

#[test]
fn verbose_search_reports_each_source_on_stderr() {
    let out = bin()
        .args(["search", "climate", "-s", "datacite,zenodo", "-v"])
        .output()
        .unwrap();
    assert_eq!(out.stdout.len(), 0);
    let notes = String::from_utf8_lossy(&out.stderr);
    assert!(notes.contains("datacite") && notes.contains("zenodo"), "{notes}");
    assert!(notes.contains("unreachable"), "{notes}");
}

#[test]
fn sources_without_their_required_key_are_skipped_not_failed() {
    let out = bin()
        .args(["search", "helmet", "-s", "roboflow", "-v"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("needs $ROBOFLOW_API_KEY"), "{stderr}");
    assert!(stderr.contains("dataseek sources"), "{stderr}");
}

// A source without its key is never asked, so the opening line does not
// count it, and says how many were left out.
#[test]
fn the_searching_line_counts_only_sources_that_will_be_asked() {
    let mut cmd = bin();
    seed_rainfall(&cmd.cache());
    let out = cmd
        .args(["search", "rainfall", "-s", "roboflow,openml", "--offline"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line = "searching 1 source for \"rainfall\", offline; \
                1 needs a key, see `dataseek doctor`";
    assert!(stderr.contains(line), "{stderr}");
}

// --offline never touches the network, so it must never mark a source as
// down either: the next online search would skip it for ten minutes.
#[test]
fn offline_with_nothing_cached_says_so_and_marks_no_outage() {
    let mut cmd = bin();
    let cache = cmd.cache();
    let out = cmd
        .args(["search", "climate", "-s", "datacite,zenodo", "--offline"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("nothing cached answers"), "{stderr}");
    let outages =
        std::fs::read_dir(cache.join("outages")).map_or(0, Iterator::count);
    assert_eq!(outages, 0);
}

/// Write a cache entry the way the binary would, `age` seconds old.
fn seed(cache: &Path, entry: &str, age: u64, value: &Value) {
    let path = cache.join(entry);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let entry = json!({
        "stored": now.saturating_sub(age),
        "version": env!("CARGO_PKG_VERSION"),
        "value": value,
    });
    std::fs::write(path, entry.to_string()).unwrap();
}

/// A cached OpenML catalog of two rainfall datasets; the newer one matches
/// the query less closely.
fn seed_rainfall(cache: &Path) {
    let rainfall = json!([
        {
            "title": "Rainfall in Lisbon",
            "url": "https://www.openml.org/d/1",
            "updated": "2001-01-01",
        },
        {
            "title": "Monthly rainfall in Porto",
            "url": "https://www.openml.org/d/2",
            "updated": "2025-06-01",
        },
    ]);
    seed(cache, "catalogs/openml.json", 0, &rainfall);
}

// The failed online search that suggests --offline marks every source down;
// the --offline retry must still answer from the cache, without -s.
#[test]
fn offline_after_an_outage_answers_from_the_cache() {
    let mut cmd = bin();
    let cache = cmd.cache();
    seed_rainfall(&cache);
    seed(&cache, "outages/openml.json", 0, &Value::Null);
    let out = cmd
        .args(["search", "rainfall", "-c", "machine-learning", "--offline"])
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let titles = result_titles(&out.stdout);
    assert!(titles.iter().any(|t| t == "Rainfall in Lisbon"), "{titles:?}");
    assert!(
        titles.iter().any(|t| t == "Monthly rainfall in Porto"),
        "{titles:?}"
    );
}

#[test]
fn sort_newest_puts_the_latest_update_first() {
    let titles = |sort: &str| {
        let mut cmd = bin();
        seed_rainfall(&cmd.cache());
        let out = cmd
            .args(["search", "rainfall", "-s", "openml", "--offline"])
            .args(["--sort", sort, "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        result_titles(&out.stdout)
    };
    // The closer title match ranks first by relevance, the later date by
    // newest, so the two orders disagree.
    assert_eq!(
        titles("relevance"),
        ["Rainfall in Lisbon", "Monthly rainfall in Porto"]
    );
    assert_eq!(
        titles("newest"),
        ["Monthly rainfall in Porto", "Rainfall in Lisbon"]
    );
}

#[test]
fn every_source_resting_says_so_and_how_to_ask_anyway() {
    let mut cmd = bin();
    seed(&cmd.cache(), "outages/datacite.json", 0, &Value::Null);
    let out = cmd
        .args(["search", "climate", "-c", "aggregator"])
        .args(["-x", "openaire,google,b2find"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("resting"), "{stderr}");
    assert!(stderr.contains("-s"), "{stderr}");
}

#[test]
fn excluding_every_chosen_source_names_the_exclusion() {
    let mut cmd = bin();
    cmd.env("DATASEEK_EXCLUDE", "zenodo");
    let out =
        cmd.args(["search", "climate", "-s", "zenodo"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("DATASEEK_EXCLUDE"), "{stderr}");
}

#[test]
fn sources_json_lists_every_source_with_docs() {
    let report = json_of(&["sources", "--json"]);
    assert_eq!(report["schema"], "dataseek-sources/1");
    let rows = report["sources"].as_array().unwrap();
    assert_ne!(rows.len(), 0);
    for row in rows {
        assert!(row["docs"].as_str().unwrap().starts_with("https://"));
    }
    let roboflow = rows.iter().find(|r| r["id"] == "roboflow").unwrap();
    assert_eq!(roboflow["key"], "missing");
    let hf = rows.iter().find(|r| r["id"] == "huggingface").unwrap();
    assert_eq!(hf["key"], "optional");
}

#[test]
fn opt_in_sources_are_marked_in_the_listing_and_doctor() {
    let report = json_of(&["sources", "--json"]);
    let rows = report["sources"].as_array().unwrap();
    let google = rows.iter().find(|r| r["id"] == "google").unwrap();
    assert!(google["opt_in"].as_str().unwrap().contains("results page"));
    let hf = rows.iter().find(|r| r["id"] == "huggingface").unwrap();
    assert!(hf["opt_in"].is_null());
    let doctor = stdout_text(&["doctor", "--plain"]);
    assert!(doctor.contains("opt-in google"), "{doctor}");
    assert!(doctor.contains("opt-in mendeley"), "{doctor}");
}

#[test]
fn piped_json_is_one_line() {
    let text = stdout_text(&["sources", "--json"]);
    assert_eq!(text.lines().count(), 1);
}

#[test]
fn a_key_from_the_environment_is_reported_but_never_printed() {
    let secret = "do-not-print-this-value";
    let mut cmd = bin();
    cmd.env("ROBOFLOW_API_KEY", secret);
    let sources = cmd.args(["sources", "--json"]).output().unwrap();
    let text = String::from_utf8_lossy(&sources.stdout);
    assert!(text.contains("\"key\":\"set\""), "{text:.300}");
    assert!(!text.contains(secret));

    let mut cmd = bin();
    cmd.env("ROBOFLOW_API_KEY", secret);
    let doctor = cmd.args(["doctor", "--json"]).output().unwrap();
    let text = String::from_utf8_lossy(&doctor.stdout);
    assert!(text.contains("$ROBOFLOW_API_KEY"), "{text}");
    assert!(!text.contains(secret));
}

#[test]
fn plain_sources_are_tab_separated() {
    let text = stdout_text(&["sources", "--plain"]);
    for line in text.lines() {
        assert_eq!(line.split('\t').count(), 7, "not seven fields: {line}");
    }
}

#[test]
fn plain_doctor_uses_ascii_marks() {
    let text = stdout_text(&["doctor", "--plain"]);
    assert!(text.contains("+ version"), "{text}");
    assert!(text.contains("ascii (--plain)"), "{text}");
    assert!(
        !text.contains('\u{2713}') && !text.contains('\u{26a0}'),
        "--plain still emitted box glyphs:\n{text}"
    );
}

#[test]
fn unknown_subcommand_exits_two_with_clean_stdout() {
    bin()
        .arg("definitely-not-a-command")
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn piped_output_has_no_ansi() {
    let out = bin().arg("sources").output().unwrap();
    assert!(out.status.success());
    assert!(
        !out.stdout.contains(&0x1b),
        "ANSI escape leaked into piped stdout"
    );
}

#[test]
fn cache_info_reports_the_budget() {
    let info = json_of(&["cache", "info", "--json"]);
    assert_eq!(info["files"], 0);
    assert_eq!(info["budget_bytes"], 30 * 1024 * 1024);
}

#[test]
fn cache_clear_succeeds_on_an_empty_cache() {
    bin().args(["cache", "clear"]).assert().success();
}

// The dry run reports exactly what the real run then does, and removes
// nothing itself.
#[test]
fn cache_clear_dry_run_touches_nothing() {
    let mut cmd = bin();
    let entry = cmd.cache().join("queries").join("k.json");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(&entry, "{}").unwrap();
    let dry =
        cmd.args(["cache", "clear", "--dry-run", "--json"]).output().unwrap();
    assert!(dry.status.success());
    assert!(entry.exists(), "--dry-run deleted the cache");
    let dry: Value = serde_json::from_slice(&dry.stdout).unwrap();

    let real =
        sandboxed(&assert_cmd::cargo::cargo_bin("dataseek"), cmd.dir.path())
            .args(["cache", "clear", "--json"])
            .output()
            .unwrap();
    assert!(real.status.success());
    assert!(!entry.exists());
    let real: Value = serde_json::from_slice(&real.stdout).unwrap();
    assert_eq!(dry["files"], 1);
    assert_eq!(dry["files"], real["files"]);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(real["dry_run"], false);
}

// --cache-dir can name a directory the user keeps other things in.
#[test]
fn cache_clear_leaves_other_files_in_the_cache_dir() {
    let mut cmd = bin();
    let dir = cmd.dir.path().join("mine");
    let theirs = dir.join("thesis.tex");
    let entry = dir.join("queries").join("k.json");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "keep").unwrap();
    std::fs::write(&entry, "{}").unwrap();
    cmd.args(["cache", "clear", "--cache-dir"]).arg(&dir).assert().success();
    assert!(theirs.exists(), "cache clear deleted a file it did not write");
    assert!(!entry.exists());
}

/// The ids `cache warm` downloads: every source searched locally.
fn catalog_ids() -> Vec<String> {
    let report = json_of(&["sources", "--json"]);
    let rows = report.get("sources").and_then(Value::as_array).unwrap();
    rows.iter()
        .filter(|r| r["search"] == "local")
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn cache_warm_dry_run_lists_catalogs_and_downloads_none() {
    let mut cmd = bin();
    let cache = cmd.cache();
    let out = cmd.args(["cache", "warm", "--dry-run"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), catalog_ids().len(), "{text}");
    assert!(text.lines().all(|l| l.ends_with("would download")), "{text}");
    assert!(!cache.exists(), "--dry-run created the cache");
}

// Some catalogs failing is a failed run: each named on stderr, the count as
// the last line, exit 1. Under -q the Error line alone still names them.
#[test]
fn cache_warm_with_failures_says_how_many_and_exits_one() {
    let ids = catalog_ids();
    let out = bin().args(["cache", "warm", "-q"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "failures went to stdout");
    let stderr = String::from_utf8(out.stderr).unwrap();
    let error = stderr.lines().find(|l| l.starts_with("Error:")).unwrap();
    let count = format!("{n} of {n} catalogs failed", n = ids.len());
    assert!(error.contains(&count), "{stderr}");
    for id in &ids {
        assert!(error.contains(id.as_str()), "{id} not named: {stderr}");
    }
}

#[test]
fn cache_warm_into_an_unwritable_dir_fails_before_downloading() {
    let mut cmd = bin();
    let file = cmd.dir.path().join("not-a-dir");
    std::fs::write(&file, "").unwrap();
    let out = cmd
        .args(["cache", "warm", "--cache-dir"])
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("cannot write the cache"), "{stderr}");
    assert!(!stderr.contains("downloading"), "{stderr}");
}

#[test]
fn inspect_reports_an_unreachable_page() {
    let out = bin()
        .args(["inspect", "https://example.org/dataset"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout.len(), 0);
    let stderr = String::from_utf8(out.stderr).unwrap();
    let (error, cause, hint) = (
        stderr.find("Error: cannot reach").expect(&stderr),
        stderr.find("  Cause: ").expect(&stderr),
        stderr.find("  Try: ").expect(&stderr),
    );
    assert!(error < cause && cause < hint, "{stderr}");
}

/// A page whose JSON-LD describes one dataset and the file it offers.
const DATASET_PAGE: &str = r#"<html><head>
<script type="application/ld+json">
{"@context": "https://schema.org", "@type": "Dataset", "name": "Rainfall",
 "distribution": [{"@type": "DataDownload", "name": "rain.csv",
  "encodingFormat": "text/csv", "contentSize": "2 MB",
  "contentUrl": "https://example.org/rain.csv"}]}
</script>
</head></html>"#;

/// The same dataset with no file list.
const BARE_PAGE: &str = r#"<html><head>
<script type="application/ld+json">
{"@context": "https://schema.org", "@type": "Dataset", "name": "Rainfall"}
</script>
</head></html>"#;

/// A file list of two entries that name nothing dataseek can show.
const UNREADABLE_FILES_PAGE: &str = r#"<html><head>
<script type="application/ld+json">
{"@context": "https://schema.org", "@type": "Dataset", "name": "Rainfall",
 "distribution": [{"@type": "DataDownload"},
  {"@type": "DataDownload", "description": "daily totals"}]}
</script>
</head></html>"#;

/// A file whose size is written in words.
const ROUGH_SIZE_PAGE: &str = r#"<html><head>
<script type="application/ld+json">
{"@context": "https://schema.org", "@type": "Dataset", "name": "Rainfall",
 "distribution": [{"@type": "DataDownload", "name": "rain.csv",
  "contentSize": "about 2 MB", "contentUrl": "https://example.org/rain.csv"}]}
</script>
</head></html>"#;

/// A page whose metadata carries terminal escapes, a bell and a C1 control
/// beside the line breaks and tabs a description may hold.
const ESCAPING_PAGE: &str = r#"<html><head>
<script type="application/ld+json">
{"@context": "https://schema.org", "@type": "Dataset",
 "name": "Rain\u001b[2Jfall\u009b", "files": "the page's own",
 "description": "daily\ntotals\tin mm\u0007"}
</script>
</head></html>"#;

/// A local server answering every request with `page`; its address.
fn serve_page(page: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = answer(stream, page);
        }
    });
    url
}

/// Read one request head and answer it with `page` as HTML.
fn answer(
    mut stream: impl std::io::Read + std::io::Write,
    page: &str,
) -> std::io::Result<()> {
    use std::io::BufRead;

    for line in std::io::BufReader::new(&mut stream).lines() {
        if line?.is_empty() {
            break;
        }
    }
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    )?;
    stream.flush()
}

/// `inspect` on a local page, with no proxy in the way.
fn inspect_page(page: &'static str, args: &[&str]) -> std::process::Output {
    bin()
        .env_remove("HTTPS_PROXY")
        .env_remove("HTTP_PROXY")
        .arg("inspect")
        .arg(serve_page(page))
        .args(args)
        .output()
        .unwrap()
}

/// `inspect --json` of a local page.
fn inspect_json(page: &'static str) -> Value {
    let out = inspect_page(page, &["--json"]);
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn inspect_lists_the_files_a_page_describes() {
    let report = inspect_json(DATASET_PAGE);
    assert_eq!(
        report["files"],
        json!([{
            "name": "rain.csv",
            "format": "text/csv",
            "size_bytes": 2_000_000,
            "url": "https://example.org/rain.csv",
        }])
    );

    let out = inspect_page(DATASET_PAGE, &[]);
    let text = String::from_utf8(out.stdout).unwrap();
    let line = format!(
        "{:<12} rain.csv  text/csv  2.0 MB  https://example.org/rain.csv",
        "files"
    );
    assert!(text.lines().any(|l| l == line), "{text}");
}

#[test]
fn inspect_says_when_a_page_lists_no_files() {
    let out = inspect_page(BARE_PAGE, &["--json"]);
    assert!(out.status.success(), "{out:?}");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["files"], json!([]));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("the page's metadata lists no files"), "{stderr}");
}

#[test]
fn inspect_prints_no_control_character_from_the_page() {
    let report = inspect_json(ESCAPING_PAGE);
    assert_eq!(report["dataset"]["name"], "Rain [2Jfall ");
    assert_eq!(report["dataset"]["description"], "daily\ntotals\tin mm ");
    assert_eq!(report["dataset"]["files"], "the page's own");
    assert_eq!(report["files"], json!([]));

    let out = inspect_page(ESCAPING_PAGE, &[]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Rain [2Jfall"), "{text}");
    assert!(!text.chars().any(|c| c.is_control() && c != '\n'), "{text:?}");
}

#[test]
fn inspect_counts_the_files_it_could_not_read() {
    let out = inspect_page(UNREADABLE_FILES_PAGE, &[]);
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("the page lists 2 files dataseek could not read"),
        "{stderr}"
    );
}

#[test]
fn inspect_prints_a_size_it_cannot_read_as_written() {
    let out = inspect_page(ROUGH_SIZE_PAGE, &[]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let line = format!(
        "{:<12} rain.csv  about 2 MB  https://example.org/rain.csv",
        "files"
    );
    assert!(text.lines().any(|l| l == line), "{text}");
}

/// A root certificate generated for one test, like the one a TLS-inspecting
/// proxy re-signs every connection under.
#[cfg(target_os = "linux")]
fn generated_root() -> rcgen::CertifiedIssuer<'static, rcgen::KeyPair> {
    let mut params = rcgen::CertificateParams::new(Vec::new()).unwrap();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    rcgen::CertifiedIssuer::self_signed(
        params,
        rcgen::KeyPair::generate().unwrap(),
    )
    .unwrap()
}

/// A local HTTPS server answering every request with `page`, under a
/// certificate for 127.0.0.1 that `root` signed; its address.
#[cfg(target_os = "linux")]
fn serve_tls(
    root: &rcgen::CertifiedIssuer<'_, rcgen::KeyPair>,
    page: &'static str,
) -> String {
    use std::sync::Arc;

    use rustls::pki_types::PrivatePkcs8KeyDer;

    let key = rcgen::KeyPair::generate().unwrap();
    let leaf = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()])
        .unwrap()
        .signed_by(&key, root)
        .unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap(),
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("https://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for tcp in listener.incoming().flatten() {
            let tls = rustls::ServerConnection::new(Arc::clone(&config));
            let _ = answer(rustls::StreamOwned::new(tls.unwrap(), tcp), page);
        }
    });
    url
}

/// `inspect --json` of `url` with `roots`, PEM text, as the whole trust
/// store: on Linux `SSL_CERT_FILE` replaces the system's.
#[cfg(target_os = "linux")]
fn inspect_trusting(roots: &str, url: &str) -> std::process::Output {
    let mut cmd = bin();
    let file = cmd.dir.path().join("roots.pem");
    std::fs::write(&file, roots).unwrap();
    cmd.env("SSL_CERT_FILE", &file)
        .env_remove("SSL_CERT_DIR")
        .env_remove("HTTPS_PROXY")
        .env_remove("HTTP_PROXY")
        .args(["inspect", url, "--json"])
        .output()
        .unwrap()
}

/// A run that failed on a certificate: nothing on stdout, and a hint that
/// says how to give the system the roots it lacks.
#[cfg(target_os = "linux")]
fn assert_refused_certificate(out: &std::process::Output) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert!(stderr.contains("cannot verify the certificate"), "{stderr}");
    assert!(stderr.contains("ca-certificates"), "{stderr}");
}

/// A page served under a private root, as a TLS-inspecting proxy serves
/// every page, is trusted once that root is in the system's store.
#[cfg(target_os = "linux")]
#[test]
fn linux_trusts_the_root_certificates_the_system_names() {
    let root = generated_root();
    let out = inspect_trusting(&root.pem(), &serve_tls(&root, DATASET_PAGE));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["dataset"]["name"], "Rainfall");
    assert_eq!(
        report["files"],
        json!([{
            "name": "rain.csv",
            "format": "text/csv",
            "size_bytes": 2_000_000,
            "url": "https://example.org/rain.csv",
        }])
    );
}

/// The same server under a root the store lacks, as behind a proxy whose
/// root was never installed.
#[cfg(target_os = "linux")]
#[test]
fn linux_refuses_a_root_the_system_does_not_name() {
    let (trusted, other) = (generated_root(), generated_root());
    let out =
        inspect_trusting(&trusted.pem(), &serve_tls(&other, DATASET_PAGE));
    assert_refused_certificate(&out);
}

/// A bare container image has no CA bundle; an empty one stands in for it.
#[cfg(target_os = "linux")]
#[test]
fn linux_with_no_roots_says_how_to_install_them() {
    let root = generated_root();
    let out = inspect_trusting("", &serve_tls(&root, DATASET_PAGE));
    assert_refused_certificate(&out);
}

#[test]
fn doctor_reports_ready_with_clean_pipe() {
    let out = bin().arg("doctor").output().unwrap();
    assert!(out.status.success());
    assert!(
        !out.stdout.contains(&0x1b),
        "doctor leaked ANSI into a piped stdout"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&format!("dataseek {}", env!("CARGO_PKG_VERSION"))));
    // Exactly "Ready.", since "Ready, with notes above." also contains "Ready".
    assert!(text.contains("Ready."), "{text}");
    // The checks take milliseconds; there is nothing to narrate.
    assert!(
        out.stderr.is_empty(),
        "doctor narrated: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(unix)]
#[test]
fn doctor_flags_a_key_file_other_users_can_read() {
    use std::os::unix::fs::PermissionsExt;

    let mut cmd = bin();
    let kaggle = tempfile::tempdir().unwrap();
    let token = kaggle.path().join("access_token");
    std::fs::write(&token, "secret-token").unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o644))
        .unwrap();
    cmd.env("KAGGLE_CONFIG_DIR", kaggle.path());
    let out = cmd.arg("doctor").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("chmod 600"), "{text}");
    assert!(!text.contains("secret-token"));
}

// A vanished consumer must not produce a panic or an error message. Asserts
// the outcome: SIGPIPE death (13) or report()'s quiet exit 0; gutting
// restore_sigpipe leaves it green, but only the signal stops a long write.
// Output is smaller than the pipe buffer, so `| head` never fails a write;
// closing the read end does.
#[cfg(unix)]
#[test]
fn a_closed_stdout_dies_quietly() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command as StdCommand, Stdio};

    let mut child = StdCommand::new(env!("CARGO_BIN_EXE_dataseek"))
        .args(["completion", "zsh"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();

    assert!(
        out.status.success() || out.status.signal() == Some(13),
        "expected a quiet death, got {:?}",
        out.status
    );
    assert!(
        out.stderr.is_empty(),
        "a closed pipe produced an error message: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn doctor_json_is_valid_and_reports_ready() {
    let report = json_of(&["doctor", "--json"]);
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["ready"], true);
    assert!(report["checks"].as_array().is_some_and(|c| !c.is_empty()));
}

#[test]
fn the_three_directories_are_distinct_and_app_scoped() {
    let report = json_of(&["doctor", "--json"]);
    let detail = |label: &str| {
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["label"] == label)
            .unwrap_or_else(|| panic!("no {label} check"))["detail"]
            .as_str()
            .unwrap()
            .to_owned()
    };

    let (config, cache, state) =
        (detail("config"), detail("cache"), detail("state"));
    // Windows aliases config_dir to data_dir and has no state_dir, so these
    // collapse unless paths.rs nests.
    assert_ne!(config, state, "config and state are the same directory");
    assert_ne!(config, cache, "config and cache are the same directory");
    for (label, path) in
        [("config", &config), ("cache", &cache), ("state", &state)]
    {
        assert!(
            path.contains(env!("CARGO_PKG_NAME")),
            "{label} is not scoped to the app name: {path}"
        );
    }
}

#[test]
fn cache_dir_moves_the_cache() {
    let elsewhere = tempfile::tempdir().unwrap();
    let dir = elsewhere.path().to_str().unwrap();
    let info = json_of(&["cache", "info", "--json", "--cache-dir", dir]);
    assert_eq!(info["path"], dir);
}

#[test]
fn man_renders_roff_with_the_manifest_version() {
    let out = bin().arg("man").output().unwrap();
    assert!(out.status.success());
    let roff = String::from_utf8_lossy(&out.stdout);
    // .TH means a real page, not help text; the version ties it to Cargo.toml.
    assert!(roff.starts_with(".ie"), "not roff output: {roff:.40}");
    assert!(roff.contains(".TH"));
    assert!(roff.contains(env!("CARGO_PKG_VERSION")));
    assert!(!out.stdout.contains(&0x1b), "ANSI leaked into the man page");
}

// The page has to render, not only exist, and say what --help says.
#[test]
fn the_man_page_renders_and_lists_every_command() {
    let Some(mut renderer) = tool("mandoc") else { return };
    // Errors only: clap_mangen's output draws style warnings (the date
    // field, paragraph macros) that are not ours to fix.
    renderer.args(["-T", "utf8", "-W", "error"]);
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("dataseek.1");
    std::fs::write(&page, stdout_text(&["man"])).unwrap();
    let out = renderer.arg(&page).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    let help = json_of(&["help", "--json"]);
    for command in help["command"]["commands"].as_array().unwrap() {
        let name = command["name"].as_str().unwrap();
        assert!(text.contains(name), "{name} missing from the man page");
    }
}

// Every option says what it does, the flags `-h` leaves out included, and
// the closing text is split into the sections a man page reader expects.
#[test]
fn the_man_page_describes_every_option_in_named_sections() {
    let roff = stdout_text(&["man"]);
    let lines: Vec<&str> = roff.lines().collect();
    for pair in lines.windows(2) {
        if pair[0].starts_with("\\fB\\-") {
            assert!(!pair[1].trim().is_empty(), "no description: {}", pair[0]);
        }
    }
    for section in
        [".SH EXAMPLES", ".SH \"EXIT STATUS\"", ".SH \"REPORTING BUGS\""]
    {
        assert!(roff.contains(section), "{section} missing");
    }
    assert!(!roff.contains(".SH EXTRA"), "{roff}");
}

fn stdout_of(args: &[&str]) -> Vec<u8> {
    bin().arg("sources").args(args).output().unwrap().stdout
}

#[test]
fn color_never_and_no_color_suppress_ansi() {
    assert!(!stdout_of(&["--color=never"]).contains(&0x1b));
    assert!(!stdout_of(&["--no-color"]).contains(&0x1b));
}

#[test]
fn color_always_forces_ansi_through_a_pipe() {
    // Captured stdout is not a TTY; --color=always must color it anyway.
    assert!(
        stdout_of(&["--color=always"]).contains(&0x1b),
        "--color=always should emit ANSI even when piped"
    );
}

/// stdout and stderr of `args` run on a pseudo-terminal, through script(1),
/// whose argv differs between BSD and util-linux.
#[cfg(unix)]
fn on_a_terminal(args: &[&str], no_color: bool) -> Vec<u8> {
    let program = env!("CARGO_BIN_EXE_dataseek");
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = sandboxed(Path::new("script"), dir.path());
    if cfg!(target_os = "linux") {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        cmd.args(["-qec", &line, "/dev/null"]);
    } else {
        cmd.args(["-q", "/dev/null", program]).args(args);
    }
    cmd.env("TERM", "xterm-256color")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("CI")
        .stdin(std::process::Stdio::null());
    if no_color {
        cmd.env("NO_COLOR", "1");
    }
    cmd.output().unwrap().stdout
}

// Color is the default on a terminal and every off switch reaches every
// writer, clap's own errors included.
#[cfg(unix)]
#[test]
fn a_terminal_gets_color_unless_told_otherwise() {
    assert!(on_a_terminal(&["sources"], false).contains(&0x1b));
    assert!(on_a_terminal(&[], false).contains(&0x1b));
    for args in [
        &["sources", "--no-color"][..],
        &["--no-color"],
        &["--color", "never", "no-such-command"],
        &["--plain", "search"],
    ] {
        let out = on_a_terminal(args, false);
        assert!(
            !out.windows(2).any(|w| w == b"\x1b["),
            "{args:?} colored:\n{}",
            String::from_utf8_lossy(&out)
        );
    }
    let out = on_a_terminal(&["sources"], true);
    assert!(!out.windows(2).any(|w| w == b"\x1b["), "NO_COLOR ignored");
}

#[test]
fn help_topics_explain_the_environment_and_exit_codes() {
    let env = stdout_text(&["help", "environment"]);
    for var in OWN_VARS.iter().chain(&KEY_VARS) {
        assert!(env.contains(var), "{var} missing:\n{env}");
    }
    let search = stdout_text(&["help", "search"]);
    assert!(search.contains("Usage: dataseek search"), "{search}");
    bin().args(["help", "no-such-topic"]).assert().code(2);
}

#[test]
fn help_json_describes_the_surface() {
    let surface = json_of(&["help", "--json"]);
    assert_eq!(surface["schema"], "dataseek-surface/1");
    // An agent's first probe, a bare call, gets the same object.
    assert_eq!(json_of(&["--json"]), surface);
    assert_eq!(surface["version"], env!("CARGO_PKG_VERSION"));
    let names: Vec<&str> = surface["command"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"search") && names.contains(&"help"));
    assert_eq!(surface["exit_codes"].as_array().unwrap().len(), 4);
}

// One command, asked alone or as part of the surface, is one description:
// its own flags, with the global ones listed once, on the root.
#[test]
fn help_for_one_command_matches_its_entry_in_the_surface() {
    let surface = json_of(&["help", "--json"]);
    for entry in surface["command"]["commands"].as_array().unwrap() {
        let name = entry["name"].as_str().unwrap();
        let alone = json_of(&["help", name, "--json"]);
        assert_eq!(alone["schema"], "dataseek-command/1");
        assert_eq!(&alone["command"], entry, "help {name} --json differs");
    }
}

/// SIGINT to a search waiting on a host that accepted the connection and
/// never answers must kill it as the shell expects: by the signal, which the
/// shell reports as 130.
#[cfg(unix)]
fn assert_ctrl_c_interrupts() {
    use std::os::unix::process::ExitStatusExt;

    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", silent.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mut child =
        sandboxed(Path::new(env!("CARGO_BIN_EXE_dataseek")), dir.path())
            .env("HTTPS_PROXY", &proxy)
            .env("HTTP_PROXY", &proxy)
            .args(["search", "climate", "-s", "zenodo", "--timeout", "0"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let kill = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(2), "{status:?}");
}

// Every code `help exit-codes` documents is one this suite reaches.
#[test]
fn every_documented_exit_code_is_asserted() {
    let codes: Vec<i32> = stdout_text(&["help", "exit-codes"])
        .lines()
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
        .collect();
    assert_eq!(codes, [0, 1, 2, 130]);
    for code in codes {
        match code {
            0 => bin().arg("sources").assert().code(0),
            1 => bin().args(["search", "x", "-s", "zenodo"]).assert().code(1),
            2 => bin().arg("--no-such-flag").assert().code(2),
            130 => {
                #[cfg(unix)]
                assert_ctrl_c_interrupts();
                continue;
            }
            other => panic!("exit code {other} is documented, never asserted"),
        };
    }
}

/// The words of a shell command line, honoring single and double quotes.
fn shell_words(line: &str) -> Vec<String> {
    let (mut words, mut word, mut quote, mut quoted) =
        (Vec::new(), String::new(), None, false);
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => (quote, quoted) = (Some(c), true),
            (None, c) if c.is_whitespace() => {
                if !word.is_empty() || quoted {
                    words.push(std::mem::take(&mut word));
                    quoted = false;
                }
            }
            (_, c) => word.push(c),
        }
    }
    if !word.is_empty() || quoted {
        words.push(word);
    }
    words
}

/// The `dsk ...` command of an example line: before any pipe, annotation or
/// comment.
fn example_command(line: &str) -> Option<Vec<String>> {
    let line = line.trim();
    let command = line.split(" | ").next()?.split("  ").next()?;
    let words = shell_words(command);
    (words.first().map(String::as_str) == Some("dsk")).then_some(words)
}

// An example that does not parse teaches the wrong thing. Each one runs with
// -h appended, which parses every argument before it and runs nothing.
#[test]
fn every_example_in_help_and_readme_parses() {
    let mut examples = Vec::new();
    for args in
        [&["-h"][..], &["search", "-h"], &["bench", "-h"], &["inspect", "-h"]]
    {
        let help = stdout_text(args);
        let block = help.split("Examples:\n").nth(1).unwrap_or_default();
        examples.extend(block.lines().filter_map(example_command));
    }
    let readme = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"),
    )
    .unwrap();
    examples.extend(readme.lines().filter_map(example_command));
    assert!(examples.len() >= 10, "{examples:?}");
    for words in examples {
        let out = bin().args(words.iter().skip(1)).arg("-h").output().unwrap();
        assert!(out.status.success(), "{words:?}: {out:?}");
    }
}

// Snapshot of the full --help surface; update with `cargo insta review`.
#[test]
fn help_snapshot() {
    let out = bin().arg("--help").output().unwrap();
    filters::with_snapshot_filters(|| {
        insta::assert_snapshot!(String::from_utf8_lossy(&out.stdout));
    });
}

// The --json shapes are a contract (`schema` tags them); a change here is a
// change for every script that reads them.
/// `search --json` over the seeded catalog, timings zeroed.
fn seeded_search_json() -> Value {
    let mut cmd = bin();
    seed_rainfall(&cmd.cache());
    let out = cmd
        .args(["search", "rainfall", "-s", "openml", "--offline", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let mut report: Value = serde_json::from_slice(&out.stdout).unwrap();
    let sources = report.get_mut("sources").and_then(Value::as_array_mut);
    for source in sources.unwrap() {
        source.as_object_mut().unwrap().insert("ms".into(), json!(0));
    }
    report
}

#[test]
fn json_shapes_snapshot() {
    let sources = json_of(&["sources", "--json"]);
    let shapes = json!({
        "sources": {
            "schema": sources["schema"],
            "sources[0]": sources["sources"][0],
        },
        "doctor": json_of(&["doctor", "--json"]),
        "cache info": json_of(&["cache", "info", "--json"]),
        "cache warm --dry-run": {
            "schema": json_of(&["cache", "warm", "--dry-run", "--json"])["schema"],
        },
        "help exit-codes": json_of(&["help", "exit-codes", "--json"]),
        "help search": {
            "schema": json_of(&["help", "search", "--json"])["schema"],
        },
        "search": seeded_search_json(),
        "inspect": inspect_json(DATASET_PAGE),
    });
    filters::with_snapshot_filters(|| {
        insta::assert_snapshot!(
            serde_json::to_string_pretty(&shapes).unwrap()
        );
    });
}

/// A `dataseek mcp` process in the sandbox at `dir`, spoken to one line at
/// a time, as an MCP client speaks to it.
struct Mcp {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
}

impl Mcp {
    /// Every request the server or its children send goes to `proxy`.
    fn start(dir: &Path, proxy: &str) -> Self {
        Self::start_as(Path::new(env!("CARGO_BIN_EXE_dataseek")), dir, proxy)
    }

    /// The server as `program`, a copy of the binary under test.
    fn start_as(program: &Path, dir: &Path, proxy: &str) -> Self {
        use std::io::BufRead;
        use std::process::Stdio;

        let mut child = sandboxed(program, dir)
            .env("HTTPS_PROXY", proxy)
            .env("HTTP_PROXY", proxy)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let lines =
            std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        Self { child, stdin, lines }
    }

    fn send(&mut self, message: &Value) {
        self.send_line(message.to_string().as_bytes());
    }

    fn send_line(&mut self, line: &[u8]) {
        use std::io::Write;

        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// The next line on stdout, which must be a JSON-RPC message.
    fn receive(&mut self) -> Value {
        let line = self.lines.next().unwrap().unwrap();
        let message: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(message.get("jsonrpc"), Some(&json!("2.0")), "{line}");
        message
    }

    fn request(&mut self, id: u64, method: &str, params: Value) {
        let mut request =
            json!({ "jsonrpc": "2.0", "id": id, "method": method });
        request.as_object_mut().unwrap().insert("params".into(), params);
        self.send(&request);
    }

    /// Send a request and read its response, which must be the next line.
    fn ask(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.request(id, method, params);
        let response = self.receive();
        assert_eq!(response.get("id"), Some(&json!(id)), "{response}");
        response
    }

    fn call(&mut self, id: u64, tool: &str, arguments: Value) -> Value {
        let mut params = json!({ "name": tool });
        params.as_object_mut().unwrap().insert("arguments".into(), arguments);
        let response = self.ask(id, "tools/call", params);
        response
            .get("result")
            .filter(|result| result.is_object())
            .cloned()
            .unwrap_or_else(|| panic!("no result: {response}"))
    }

    /// Close stdin, as a client shutting the server down does; the server
    /// must exit at once, having written nothing more. Returns its stderr.
    fn close(mut self) -> String {
        use std::io::Read;

        drop(self.stdin.take());
        let status = exit_within_10s(&mut self.child);
        assert!(status.success(), "{status:?}");
        assert!(self.lines.next().is_none(), "stdout carried more lines");
        let mut stderr = String::new();
        self.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        stderr
    }
}

/// Wait for `child` to exit, killing it and failing after ten seconds.
fn exit_within_10s(
    child: &mut std::process::Child,
) -> std::process::ExitStatus {
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if started.elapsed() > std::time::Duration::from_secs(10) {
            child.kill().unwrap();
            panic!("dataseek mcp did not exit");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A search over MCP that waits on `proxy`, which accepts its connection
/// and never answers, for up to a minute.
fn waiting_search(mcp: &mut Mcp, id: u64) {
    mcp.request(
        id,
        "tools/call",
        json!({
            "name": "search",
            "arguments": {
                "query": "climate",
                "source": ["zenodo"],
                "timeout": 60,
            },
        }),
    );
}

/// A proxy on 127.0.0.1 that answers every request with `page`, whether the
/// client tunnels through it with CONNECT or not. Returns its address.
fn serving(page: &'static str) -> String {
    use std::io::{BufRead, Write};

    fn head(reader: &mut impl BufRead) -> String {
        let mut head = String::new();
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
            head.push_str(&line);
            line.clear();
        }
        head
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Ok(clone) = stream.try_clone() else { continue };
            let mut reader = std::io::BufReader::new(clone);
            if head(&mut reader).starts_with("CONNECT") {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                head(&mut reader);
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    });
    address
}

// The session a desktop client runs: the handshake, the tool list, a call
// of each tool and a failing one, then shutdown. Each result is the object
// the command's --json prints, and stdout carries nothing but responses.
#[test]
fn an_mcp_client_drives_a_whole_session_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    seed_rainfall(&dir.path().join("cache").join("dataseek"));
    let proxy = serving(include_str!("fixtures/inspect/zenodo.html"));
    let mut mcp = Mcp::start(dir.path(), &proxy);

    let init = mcp.ask(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" },
        }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(init["result"]["capabilities"], json!({ "tools": {} }));
    assert_eq!(
        init["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    mcp.send(
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );

    let list = mcp.ask(2, "tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["search", "sources", "inspect"]);

    let sources = mcp.call(3, "sources", json!({}));
    assert_eq!(sources["isError"], false);
    assert_eq!(sources["structuredContent"], json_of(&["sources", "--json"]));
    let text = sources["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        sources["structuredContent"]
    );

    let search = mcp.call(
        4,
        "search",
        json!({ "query": "rainfall", "source": ["openml"], "offline": true }),
    );
    assert_eq!(search["isError"], false, "{search}");
    let mut report = search["structuredContent"].clone();
    for source in report["sources"].as_array_mut().unwrap() {
        source["ms"] = json!(0);
    }
    assert_eq!(report, seeded_search_json());

    let url = "http://zenodo.org/records/13135140";
    let inspect = mcp.call(5, "inspect", json!({ "url": url }));
    assert_eq!(inspect["isError"], false, "{inspect}");
    let page = &inspect["structuredContent"];
    assert_eq!(page["schema"], "dataseek-inspect/1");
    assert_eq!(page["url"], url);
    assert_eq!(
        page["dataset"]["name"],
        "Evaluation of the influence of rain on air surface temperature \
         measurements"
    );
    assert_eq!(
        page["dataset"]["identifier"],
        "https://doi.org/10.5281/zenodo.13135140"
    );

    let failed = mcp.call(
        6,
        "search",
        json!({ "query": "climate", "source": ["not-a-source"] }),
    );
    assert_eq!(failed["isError"], true);
    let text = failed["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("Error: invalid value 'not-a-source' for '--source")
            && text.contains("\n  Try:"),
        "{text}"
    );
    assert_eq!(failed["structuredContent"]["event"], "error");

    let stderr = mcp.close();
    assert!(stderr.contains("searching 1 source"), "{stderr}");
}

// The stateless revision: no handshake, every request names its version.
#[test]
fn an_mcp_client_on_the_current_revision_needs_no_handshake() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), "http://127.0.0.1:9");
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "1" },
    });

    let discover = mcp.ask(1, "server/discover", json!({ "_meta": meta }));
    let result = &discover["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["supportedVersions"][0], "2026-07-28");
    assert_eq!(result["cacheScope"], "public");
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "dataseek"
    );

    let sources =
        mcp.ask(2, "tools/call", json!({ "name": "sources", "_meta": meta }));
    assert_eq!(sources["result"]["resultType"], "complete");
    assert_eq!(
        sources["result"]["structuredContent"]["schema"],
        "dataseek-sources/1"
    );

    let mut old = meta.clone();
    old["io.modelcontextprotocol/protocolVersion"] = json!("1900-01-01");
    let refused = mcp.ask(3, "tools/list", json!({ "_meta": old }));
    assert_eq!(refused["error"]["code"], -32022);
    assert_eq!(refused["error"]["data"]["requested"], "1900-01-01");

    let stderr = mcp.close();
    assert!(stderr.contains("serving search, sources, inspect"), "{stderr}");
}

// A line that is not UTF-8 is a parse error like any other, and the session
// goes on.
#[test]
fn an_mcp_line_that_is_not_utf8_leaves_the_session_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), "http://127.0.0.1:9");
    mcp.send_line(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\"}");
    let refused = mcp.receive();
    assert_eq!(refused["error"]["code"], -32700, "{refused}");
    assert_eq!(refused["id"], Value::Null);
    let ping = mcp.ask(2, "ping", json!({}));
    assert_eq!(ping["result"]["resultType"], "complete");
    mcp.close();
}

// A client that stops reading turns the server's next answer into a write
// error rather than a SIGPIPE death, so the server still kills the call it
// was running before it exits.
#[cfg(unix)]
#[test]
fn an_mcp_server_whose_client_stops_reading_ends_its_calls() {
    use std::io::{Read, Write};
    use std::os::unix::process::ExitStatusExt;

    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", silent.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), &proxy);
    waiting_search(&mut mcp, 1);
    let (mut search, _) = silent.accept().unwrap();

    let Mcp { mut child, stdin, lines } = mcp;
    drop(lines);
    let mut stdin = stdin.unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"ping"}}"#).unwrap();
    stdin.flush().unwrap();
    let status = exit_within_10s(&mut child);
    drop(stdin);
    assert_eq!(status.signal(), None, "{status:?}");
    assert!(status.success(), "{status:?}");

    search.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let read = search.read(&mut [0; 512]);
    assert!(
        !read.as_ref().is_err_and(|e| matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        )),
        "the search outlived the server: {read:?}"
    );
}

// A value clap refuses comes back with clap's list of the valid ones, and a
// hint that does not send the model to --help.
#[test]
fn an_mcp_value_clap_refuses_comes_back_with_the_valid_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), "http://127.0.0.1:9");
    let refused =
        mcp.call(1, "search", json!({ "query": "climate", "sort": "oldest" }));
    assert_eq!(refused["isError"], true);
    let text = refused["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("\n  Cause: [possible values: relevance, newest]\n"),
        "{text}"
    );
    assert!(text.contains("Try:   pass the arguments tools/list"), "{text}");
    mcp.close();
}

// An upgrade replaces the binary while the server runs. On Linux the running
// binary's own path then names a deleted file, so calls must use the path
// the server was started from.
#[cfg(target_os = "linux")]
#[test]
fn an_mcp_server_keeps_working_after_its_binary_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("dataseek");
    std::fs::copy(env!("CARGO_BIN_EXE_dataseek"), &program).unwrap();
    let mut mcp = Mcp::start_as(&program, dir.path(), "http://127.0.0.1:9");
    assert_eq!(
        mcp.ask(1, "ping", json!({}))["result"]["resultType"],
        "complete"
    );

    std::fs::remove_file(&program).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_dataseek"), &program).unwrap();
    let sources = mcp.call(2, "sources", json!({}));
    assert_eq!(sources["structuredContent"]["schema"], "dataseek-sources/1");
    mcp.close();
}

// Calls run beside the session: a ping is answered while searches wait, a
// reused id and a fifth call are refused, a cancelled call never answers,
// and closing stdin ends the calls still running.
#[test]
fn an_mcp_session_keeps_answering_while_calls_run() {
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", silent.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), &proxy);
    for id in 1..=4 {
        waiting_search(&mut mcp, id);
    }

    waiting_search(&mut mcp, 1);
    let reused = mcp.receive();
    assert_eq!(reused["id"], 1, "{reused}");
    assert_eq!(reused["error"]["code"], -32600, "{reused}");
    waiting_search(&mut mcp, 5);
    let busy = mcp.receive();
    assert_eq!(busy["id"], 5, "{busy}");
    assert_eq!(busy["result"]["isError"], true, "{busy}");
    let text = busy["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Error: 4 calls are already running"), "{text}");
    let ping = mcp.ask(6, "ping", json!({}));
    assert_eq!(ping["result"]["resultType"], "complete");

    // The cancelled call frees its place and never answers: the next line
    // is the ping's answer, and close() finds nothing after it.
    mcp.send(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": 1 },
    }));
    waiting_search(&mut mcp, 7);
    let ping = mcp.ask(8, "ping", json!({}));
    assert_eq!(ping["result"]["resultType"], "complete");
    mcp.close();
}
