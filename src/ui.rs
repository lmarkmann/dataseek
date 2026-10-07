//! The human layer: staged narration, spinners, and progress bars, all on
//! stderr, never on stdout.
//!
//! How it shows depends on who is reading stderr. A terminal gets status lines
//! plus live bars and spinners; a pipe, or `--no-progress`, gets the status
//! lines alone, so a slow command still says what it is doing without frames;
//! `--json` turns each line into an NDJSON event; `--quiet` silences it.
//! Outside Live mode the bars are hidden `ProgressBar`s, so callers never
//! branch.

use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use clap::builder::styling::Style;
use indicatif::{ProgressBar, ProgressStyle};

use crate::output::{Out, Verbosity};
use crate::palette;

/// Tag on every JSON line dataseek writes to stderr, so a consumer can tell
/// events from stray output and notice a shape change.
pub const EVENTS_SCHEMA: &str = "dataseek-events/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Mode {
    Off,
    Lines,
    Live,
    Events,
}

static MODE: AtomicU8 = AtomicU8::new(Mode::Off as u8);
static UNICODE: AtomicBool = AtomicBool::new(true);
static COLOR: AtomicBool = AtomicBool::new(false);

/// Wire the layer once, in `main`. Takes the context rather than loose bools,
/// which would transpose without a compile error.
pub fn init(out: &Out) {
    let mode = if out.verbosity == Verbosity::Quiet {
        Mode::Off
    } else if out.json {
        Mode::Events
    } else if out.progress && std::io::stderr().is_terminal() {
        Mode::Live
    } else {
        Mode::Lines
    };
    set(mode, !out.plain, out.color_on_stderr());
}

fn set(mode: Mode, unicode: bool, color: bool) {
    MODE.store(mode as u8, Ordering::Relaxed);
    UNICODE.store(unicode, Ordering::Relaxed);
    COLOR.store(color, Ordering::Relaxed);
}

fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        1 => Mode::Lines,
        2 => Mode::Live,
        3 => Mode::Events,
        _ => Mode::Off,
    }
}

fn unicode() -> bool {
    UNICODE.load(Ordering::Relaxed)
}

fn color() -> bool {
    COLOR.load(Ordering::Relaxed)
}

fn paint(style: Style, body: impl Display) -> String {
    if color() { format!("{style}{body}{style:#}") } else { body.to_string() }
}

/// One line on stderr: marked text for a person, an event for `--json`.
fn say(event: &str, indent: &str, mark: &str, msg: impl Display) {
    match mode() {
        Mode::Off => {}
        Mode::Lines | Mode::Live => {
            let _ = writeln!(anstream::stderr(), "{indent}{mark} {msg}");
        }
        Mode::Events => event_line(&serde_json::json!({
            "schema": EVENTS_SCHEMA,
            "event": event,
            "message": msg.to_string(),
        })),
    }
}

/// Write one event as a line of JSON on stderr. Also used by the error
/// printer, which runs whatever the mode is. A failed write is dropped:
/// stderr is where it would be reported.
pub fn event_line(event: &serde_json::Value) {
    let _ = writeln!(anstream::stderr(), "{event}");
}

/// Pass a line another dataseek process wrote to its stderr on to ours, as
/// written: `mcp` relays its tool calls' events this way.
pub fn relay(line: &str) {
    let _ = writeln!(anstream::stderr(), "{line}");
}

/// Open a stage of work: `> <msg>`.
pub fn stage(msg: impl Display) {
    let mark = if unicode() { "▸" } else { ">" };
    say("stage", "", &paint(palette::accent(), mark), msg);
}

/// Report a resolved fact under the stage: `  + <msg>`.
pub fn ok(msg: impl Display) {
    let mark = if unicode() { "✓" } else { "+" };
    say("ok", "  ", &paint(palette::success(), mark), msg);
}

/// Flag a soft problem under the stage: `  ! <msg>`. Hard failures go through
/// `report` in `src/lib.rs`.
pub fn warn(msg: impl Display) {
    let mark = if unicode() { "⚠" } else { "!" };
    say("warning", "  ", &paint(palette::warning(), mark), msg);
}

/// A spinner for work of unknown length. Without a live terminal the message
/// becomes a stage line instead, so the wait is still announced.
pub fn spinner(msg: impl Into<String>) -> ProgressBar {
    let msg = msg.into();
    if mode() != Mode::Live {
        stage(&msg);
        return ProgressBar::hidden();
    }
    let ticks =
        if unicode() { "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏" } else { "|/-\\" };
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template(&spinner_template())
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_chars(ticks),
    );
    pb.set_message(msg);
    pb.enable_steady_tick(Duration::from_millis(90));
    pb
}

/// A bar over `len` known units; use a [`spinner`] when the total is unknown.
/// Hidden unless a terminal is watching; the stage line before it carries the
/// news everywhere else.
pub fn bar(len: u64, msg: impl Into<String>) -> ProgressBar {
    if mode() != Mode::Live {
        return ProgressBar::hidden();
    }
    let chars = if unicode() { "█▓░" } else { "#=-" };
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template(&bar_template())
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars(chars),
    );
    pb.set_message(msg.into());
    pb
}

// Built here so the tests can parse them without a terminal.
fn spinner_template() -> String {
    if color() {
        format!(
            "  {{spinner:.{}}} {{msg}} {{elapsed}}",
            palette::accent_token()
        )
    } else {
        "  {spinner} {msg} {elapsed}".to_owned()
    }
}

fn bar_template() -> String {
    if color() {
        format!(
            "  {{msg}} [{{bar:30.{}}}] {{pos}}/{{len}} {{elapsed}}",
            palette::accent_token()
        )
    } else {
        "  {msg} [{bar:30}] {pos}/{len} {elapsed}".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    // The gates are process globals. nextest isolates tests, but plain `cargo
    // test` runs them as threads, so serialize.
    static UI: Mutex<()> = Mutex::new(());

    fn locked() -> MutexGuard<'static, ()> {
        UI.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // Only a live terminal gets frames; every other mode must get no-ops.
    #[test]
    fn bars_are_hidden_without_a_live_terminal() {
        let _guard = locked();
        for mode in [Mode::Off, Mode::Lines, Mode::Events] {
            set(mode, true, false);
            assert!(spinner("scanning").is_hidden(), "{mode:?}");
            assert!(bar(10, "counting").is_hidden(), "{mode:?}");
        }
    }

    // A bad template falls back to indicatif's default at runtime, so this is
    // the only thing that catches a typo.
    #[test]
    fn templates_parse_in_both_color_modes() {
        let _guard = locked();
        for color in [true, false] {
            set(Mode::Live, true, color);
            ProgressStyle::with_template(&spinner_template()).unwrap();
            ProgressStyle::with_template(&bar_template()).unwrap();
        }
    }
}
