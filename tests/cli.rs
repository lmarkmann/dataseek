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
    // clap's wrap_help reads COLUMNS, so help output is a function of the
    // terminal width of whoever ran the tests. Without pinning it, the
    // snapshot fails for anyone whose shell or CI image exports a different
    // value, for no reason connected to the CLI.
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
        .stdout(predicate::str::contains("count"))
        .stdout(predicate::str::contains("completion"));
}

// Naked invocation is a success, not a usage error, so it exits 0. clap routes
// this variant of help to stderr, which is worth pinning: stdout stays empty,
// so `tool | consumer` in a pipeline gets nothing rather than a help page.
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
        // main.rs passes the binary name to clap_complete as a literal.
        // Nothing else ties that string back to the manifest, so this is the
        // only check that a rename which missed it would fail.
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
        .args(["count", "--color=chartreuse", "Cargo.toml"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
}

#[test]
fn quiet_conflicts_with_verbose() {
    bin().args(["count", "-q", "-v", "Cargo.toml"]).assert().code(2);
}

#[test]
fn verbose_notes_go_to_stderr_and_leave_stdout_alone() {
    let plain = bin().args(["count", "Cargo.toml"]).output().unwrap();
    let noisy = bin().args(["count", "Cargo.toml", "-v"]).output().unwrap();

    assert_eq!(plain.stdout, noisy.stdout, "-v changed the data on stdout");
    assert!(plain.stderr.is_empty(), "a normal run narrated");
    let notes = String::from_utf8_lossy(&noisy.stderr);
    assert!(notes.contains("bytes from Cargo.toml"), "{notes}");
}

#[test]
fn plain_count_is_tab_separated() {
    let out = bin().args(["count", "--plain", "Cargo.toml"]).output().unwrap();
    let line = String::from_utf8_lossy(&out.stdout);
    let fields: Vec<_> = line.trim_end().split('\t').collect();
    assert_eq!(fields.len(), 3, "expected three tab-separated fields: {line}");
    for field in fields {
        assert!(field.parse::<usize>().is_ok(), "not a number: {field}");
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
    let out = bin().args(["count", "Cargo.toml"]).output().unwrap();
    assert!(out.status.success());
    assert!(
        !out.stdout.contains(&0x1b),
        "ANSI escape leaked into piped stdout"
    );
}

#[test]
fn count_reads_stdin() {
    bin()
        .arg("count")
        .write_stdin("alpha beta\ngamma\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("2 lines"))
        .stdout(predicate::str::contains("3 words"));
}

#[test]
fn json_output_is_valid() {
    let out = bin()
        .args(["count", "--json"])
        .write_stdin("one two\n")
        .output()
        .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(parsed["words"], 2);
    assert_eq!(parsed["lines"], 1);
}

#[test]
fn missing_file_error_prints_each_part_once() {
    let out = bin().args(["count", "does-not-exist.txt"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a runtime failure exits 1, not 2");
    assert!(out.stdout.is_empty(), "an error put bytes on stdout");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let error = stderr.find("Error:").expect("no Error: line");
    let hint = stderr.find("  Try:").expect("no Try: line");
    let cause = stderr.find("  Cause:").expect("no Cause: line");
    assert!(error < hint && hint < cause, "wrong order:\n{stderr}");

    // The chain walk in report() is what prints the cause. A message that also
    // interpolates its own {source} prints it a second time, which is what
    // this counts. Read the cause text out of the line rather than hardcoding
    // it: the OS supplies the wording, so it is "No such file or directory" on
    // Unix and "The system cannot find the file specified." on Windows.
    let cause_text = stderr[cause..]
        .lines()
        .next()
        .and_then(|line| line.trim_start().strip_prefix("Cause:"))
        .expect("no Cause: line")
        .trim();
    assert!(!cause_text.is_empty(), "empty cause:\n{stderr}");
    assert_eq!(
        stderr.matches(cause_text).count(),
        1,
        "the cause was printed more than once:\n{stderr}"
    );
    assert_eq!(
        stderr.matches("Cause:").count(),
        1,
        "more than one Cause: line:\n{stderr}"
    );
}

#[test]
fn binary_input_says_it_is_not_text() {
    // The binary this suite just built is the most convenient non-UTF-8 file
    // around.
    let out = bin()
        .args(["count", env!("CARGO_BIN_EXE_dataseek")])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not UTF-8 text"), "{stderr}");
    // The old hint sent the user to check the path or pipe it instead; both
    // are dead ends for bytes that are simply not text.
    assert!(!stderr.contains("check the path"), "{stderr}");
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
    // "Ready." exactly: "Ready, with notes above." also contains "Ready".
    assert!(text.contains("Ready."), "{text}");
    // The narration invariant, mechanically: piped stdout means no human is
    // watching, so ui::stage/ok and the progress bar must all have been
    // no-ops.
    assert!(
        out.stderr.is_empty(),
        "doctor narrated into a pipe: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// A consumer that goes away first must not produce a panic or an error
// message. Two mechanisms deliver that and this asserts the outcome rather
// than either one: SIGPIPE reset to the default in main (death by signal 13),
// and the broken-pipe arm of report() (a quiet exit 0). Measured: gutting
// restore_sigpipe leaves this green, because the second arm covers the same
// case. Both are worth keeping, since only the signal stops a long write
// mid-stream.
//
// `tool | head` will not provoke this in the template as shipped: the largest
// output is well under the pipe buffer, so no write ever fails. Closing the
// read end is the reliable way in.
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
    // On Windows the base strategy aliases config_dir to data_dir and has no
    // state_dir, so these collapse onto one directory unless paths.rs nests.
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
    // .TH is the roff title macro, so its presence means a real page rather
    // than help text; the version proves the page reads Cargo.toml like
    // --version does, instead of carrying a second hardcoded number.
    assert!(roff.starts_with(".ie"), "not roff output: {roff:.40}");
    assert!(roff.contains(".TH"));
    assert!(roff.contains(env!("CARGO_PKG_VERSION")));
    assert!(!out.stdout.contains(&0x1b), "ANSI leaked into the man page");
}

fn stdout_of(args: &[&str]) -> Vec<u8> {
    bin().args(args).arg("Cargo.toml").output().unwrap().stdout
}

#[test]
fn color_never_and_no_color_suppress_ansi() {
    assert!(!stdout_of(&["count", "--color=never"]).contains(&0x1b));
    assert!(!stdout_of(&["count", "--no-color"]).contains(&0x1b));
}

#[test]
fn color_always_forces_ansi_through_a_pipe() {
    // assert_cmd captures stdout (not a TTY); --color=always must still color,
    // matching ripgrep/fd so `tool --color=always | less -R` stays colored.
    assert!(
        stdout_of(&["count", "--color=always"]).contains(&0x1b),
        "--color=always should emit ANSI even when piped"
    );
}

// Snapshot of the full --help surface. Run `cargo insta review` to update
// after changing the CLI, and `cargo insta accept` once you have renamed the
// crate.
#[test]
fn help_snapshot() {
    let out = bin().arg("--help").output().unwrap();
    filters::with_snapshot_filters(|| {
        insta::assert_snapshot!(String::from_utf8_lossy(&out.stdout));
    });
}
