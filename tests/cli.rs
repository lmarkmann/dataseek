//! Tests of the CLI surface itself: flags, exit codes, pipe behavior, and the
//! shape of stdout. These run the built binary, not the library functions.

// A panic in a helper here is the assertion failing, which is the point.
// `clippy.toml` exempts `#[test]` bodies; these helpers sit outside one.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "cli/filters.rs"]
mod filters;

use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    let mut cmd = Command::cargo_bin("dataseek").expect("dataseek binary");
    // wrap_help reads COLUMNS; pin it so the snapshot does not depend on the
    // runner's terminal.
    cmd.env("COLUMNS", "100");
    cmd
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
        .stdout(predicate::str::contains("doctor"))
        .stdout(predicate::str::contains("completion"));
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
        // main.rs passes the binary name as a literal; tie it to the manifest.
        let script = String::from_utf8_lossy(&out.stdout);
        assert!(
            script.contains(env!("CARGO_PKG_NAME")),
            "{shell} script does not name the binary"
        );
    }
}

#[test]
fn a_bad_flag_value_exits_two() {
    bin()
        .args(["doctor", "--color=chartreuse"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
}

#[test]
fn quiet_conflicts_with_verbose() {
    bin().args(["doctor", "-q", "-v"]).assert().code(2);
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
    let out = bin().arg("doctor").output().unwrap();
    assert!(out.status.success());
    assert!(
        !out.stdout.contains(&0x1b),
        "ANSI escape leaked into piped stdout"
    );
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
    bin().arg("doctor").args(args).output().unwrap().stdout
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
