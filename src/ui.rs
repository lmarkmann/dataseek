//! The human layer: staged narration, spinners, and progress bars, all on
//! stderr and all no-ops unless a human is watching.
//!
//! stdout is the data channel, so the layer is gated off unless stderr is a
//! terminal and neither `--json` nor `--quiet` is set. Off, the bars are hidden
//! `ProgressBar`s, so callers never branch on it.

use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anstream::eprintln;
use clap::builder::styling::Style;
use indicatif::{ProgressBar, ProgressStyle};

use crate::output::Out;
use crate::palette;

static RICH: AtomicBool = AtomicBool::new(false);
static UNICODE: AtomicBool = AtomicBool::new(true);
static COLOR: AtomicBool = AtomicBool::new(false);

/// Wire the layer once, in `main`. Takes the context rather than three bools,
/// which would transpose without a compile error.
pub fn init(out: &Out) {
    RICH.store(out.stderr_is_rich(), Ordering::Relaxed);
    UNICODE.store(!out.plain, Ordering::Relaxed);
    COLOR.store(out.color_on_stderr(), Ordering::Relaxed);
}

#[cfg(test)]
fn init_for_test(rich: bool, unicode: bool, color: bool) {
    RICH.store(rich, Ordering::Relaxed);
    UNICODE.store(unicode, Ordering::Relaxed);
    COLOR.store(color, Ordering::Relaxed);
}

fn rich() -> bool {
    RICH.load(Ordering::Relaxed)
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

/// Open a stage of work: `> <msg>`.
pub fn stage(msg: impl Display) {
    if rich() {
        let mark = if unicode() { "▸" } else { ">" };
        eprintln!("{} {msg}", paint(palette::accent(), mark));
    }
}

/// Report a resolved fact under the stage: `  + <msg>`.
pub fn ok(msg: impl Display) {
    if rich() {
        let mark = if unicode() { "✓" } else { "+" };
        eprintln!("  {} {msg}", paint(palette::success(), mark));
    }
}

/// Flag a soft problem under the stage: `  ! <msg>`. Hard failures go through
/// `main::report`.
pub fn warn(msg: impl Display) {
    if rich() {
        let mark = if unicode() { "⚠" } else { "!" };
        eprintln!("  {} {msg}", paint(palette::warning(), mark));
    }
}

/// A spinner for work of unknown length. Hidden and thread-free when off.
pub fn spinner(msg: impl Into<String>) -> ProgressBar {
    if !rich() {
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
    pb.set_message(msg.into());
    pb.enable_steady_tick(Duration::from_millis(90));
    pb
}

/// A bar over `len` known units; use a [`spinner`] when the total is unknown.
pub fn bar(len: u64, msg: impl Into<String>) -> ProgressBar {
    if !rich() {
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
        format!("  {{spinner:.{}}} {{msg}}", palette::accent_token())
    } else {
        "  {spinner} {msg}".to_owned()
    }
}

fn bar_template() -> String {
    if color() {
        format!(
            "  {{msg}} [{{bar:30.{}}}] {{pos}}/{{len}}",
            palette::accent_token()
        )
    } else {
        "  {msg} [{bar:30}] {pos}/{len}".to_owned()
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

    // Off is what a non-TTY run resolves to; every bar must be a no-op.
    #[test]
    fn bars_are_hidden_when_the_layer_is_off() {
        let _guard = locked();
        init_for_test(false, true, false);
        assert!(spinner("scanning").is_hidden());
        assert!(bar(10, "counting").is_hidden());
    }

    // A bad template falls back to indicatif's default at runtime, so this is
    // the only thing that catches a typo.
    #[test]
    fn templates_parse_in_both_color_modes() {
        let _guard = locked();
        for color in [true, false] {
            init_for_test(true, true, color);
            ProgressStyle::with_template(&spinner_template()).unwrap();
            ProgressStyle::with_template(&bar_template()).unwrap();
        }
    }
}
