//! The human layer: staged narration, spinners, and progress bars. Every part
//! of it draws to stderr and is a no-op unless a human is watching.
//!
//! The invariant that keeps pipes clean: stdout is the data channel
//! (`--json | jq`, `count | wc`). Progress that leaked into it would corrupt
//! every downstream consumer, so the whole layer is gated off unless stderr is
//! a terminal and neither `--json` nor `--quiet` is set. `indicatif` does the
//! drawing; a hidden `ProgressBar` keeps the gating branch-free, so callers
//! write `ui::spinner(..)` and never `if rich { .. }`.
//!
//! Colors come from [`crate::palette`]; narration embeds anstyle escapes
//! directly, and the spinner and bar take the palette's token, which indicatif
//! parses for names, `#rrggbb`, and 256-color indices alike. Both are exact.

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

/// Wire the layer from the resolved output context once, in `main`. Takes the
/// context rather than three bools: they are all the same type, so transposing
/// two compiles cleanly and silently breaks `--plain`, and clippy's
/// `fn_params_excessive_bools` only fires above three.
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

/// Open a stage of work: `> <msg>`. The running "here is what I am doing now".
pub fn stage(msg: impl Display) {
    if rich() {
        let mark = if unicode() { "▸" } else { ">" };
        eprintln!("{} {msg}", paint(palette::accent(), mark));
    }
}

/// Report a resolved fact under the current stage: `  + <msg>`.
pub fn ok(msg: impl Display) {
    if rich() {
        let mark = if unicode() { "✓" } else { "+" };
        eprintln!("  {} {msg}", paint(palette::success(), mark));
    }
}

/// Flag a soft problem under the current stage: `  ! <msg>`. Not an error; the
/// command still succeeds. Hard failures go through the error path in `main`.
pub fn warn(msg: impl Display) {
    if rich() {
        let mark = if unicode() { "⚠" } else { "!" };
        eprintln!("  {} {msg}", paint(palette::warning(), mark));
    }
}

/// A spinner for indeterminate work (a scan, a blocking read). Finish it with
/// `.finish_and_clear()` or just drop it. Hidden and thread-free when off, so
/// it costs nothing in a pipe.
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

/// A determinate bar over `len` units of work. Call `.inc(1)` per unit. Use it
/// only when the total is genuinely known; show a [`spinner`] otherwise rather
/// than fake a percentage.
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

// Template strings are built here, not inline, so the tests below can parse
// them without a terminal and catch a typo before it ever ships.
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

    // The three gates are process globals, so these tests are not independent.
    // nextest gives each test its own process and would not notice, but `cargo
    // insta test` defaults to plain `cargo test`, which runs them as threads,
    // and that command runs inside `just rename` after the tree has already
    // been rewritten. A spurious failure there aborts the one supported clone
    // path halfway through.
    static UI: Mutex<()> = Mutex::new(());

    fn locked() -> MutexGuard<'static, ()> {
        UI.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // The contract that protects pipes: with the layer off (the default, and
    // what a non-TTY run resolves to), every bar is a no-op.
    #[test]
    fn bars_are_hidden_when_the_layer_is_off() {
        let _guard = locked();
        init_for_test(false, true, false);
        assert!(spinner("scanning").is_hidden());
        assert!(bar(10, "counting").is_hidden());
    }

    // A template that fails to parse degrades to indicatif's default style at
    // runtime rather than panicking, so this test is what actually catches a
    // typo: both must parse, colored or not, for whatever token the colorway
    // resolves the accent to.
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
