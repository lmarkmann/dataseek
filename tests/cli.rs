//! Tests of the CLI surface itself: flags, exit codes, pipe behavior, and the
//! shape of stdout. These run the built binary, not the library functions.
//!
//! Every run gets its own home, config and cache directories, so a key or a
//! cache on the developer's machine can never change an outcome, and no test
//! reaches the network: searches go through a proxy on a closed port, which
//! is how an offline machine looks to dataseek.

// A panic in a helper here is the assertion failing, which is the point.
// `clippy.toml` exempts `#[test]` bodies; these helpers sit outside one.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "cli/filters.rs"]
mod filters;

use assert_cmd::Command;
use predicates::prelude::*;

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

struct Sandbox {
    _dir: tempfile::TempDir,
    cmd: Command,
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

fn bin() -> Sandbox {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("dataseek").expect("dataseek binary");
    // wrap_help reads COLUMNS; pin it so the snapshot does not depend on the
    // runner's terminal.
    cmd.env("COLUMNS", "100")
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env("KAGGLE_CONFIG_DIR", dir.path().join("kaggle"))
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9");
    for var in KEY_VARS {
        cmd.env_remove(var);
    }
    Sandbox { _dir: dir, cmd }
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
        .stdout(predicate::str::contains("sources"));
}

// clap routes naked-invocation help to stderr; stdout stays empty so a pipeline
// gets nothing rather than a help page.
#[test]
fn naked_invocation_prints_help_to_stderr_and_exits_zero() {
    bin()
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Usage: dataseek"))
        .stderr(predicate::str::contains("Commands:"));
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
    let out = bin().args(["completion", "fish"]).output().unwrap();
    let script = String::from_utf8_lossy(&out.stdout);
    assert!(
        script.contains("huggingface"),
        "source ids missing:\n{script:.400}"
    );
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

#[test]
fn sources_json_lists_every_source_with_docs() {
    let out = bin().args(["sources", "--json"]).output().unwrap();
    assert!(out.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = rows.as_array().unwrap();
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
    let out = bin().args(["sources", "--plain"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        assert_eq!(line.split('\t').count(), 6, "not six fields: {line}");
    }
}

#[test]
fn plain_doctor_uses_ascii_marks() {
    let out = bin().args(["doctor", "--plain"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
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
    let out = bin().args(["cache", "info", "--json"]).output().unwrap();
    assert!(out.status.success());
    let info: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(info["files"], 0);
    assert_eq!(info["budget_bytes"], 30 * 1024 * 1024);
}

#[test]
fn cache_clear_succeeds_on_an_empty_cache() {
    bin().args(["cache", "clear"]).assert().success();
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
    // Piped stdout means no human is watching, so the ui layer must be silent.
    assert!(
        out.stderr.is_empty(),
        "doctor narrated into a pipe: {}",
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
    let out = bin().args(["doctor", "--json"]).output().unwrap();
    assert!(out.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["ready"], true);
    assert!(report["checks"].as_array().is_some_and(|c| !c.is_empty()));
}

#[test]
fn the_three_directories_are_distinct_and_app_scoped() {
    let out = bin().args(["doctor", "--json"]).output().unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap();
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

// Snapshot of the full --help surface; update with `cargo insta review`.
#[test]
fn help_snapshot() {
    let out = bin().arg("--help").output().unwrap();
    filters::with_snapshot_filters(|| {
        insta::assert_snapshot!(String::from_utf8_lossy(&out.stdout));
    });
}
