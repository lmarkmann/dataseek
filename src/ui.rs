//! The human layer: staged narration, spinners, and progress bars, all on
//! stderr, never on stdout.
//!
//! How it shows depends on who is reading stderr. A terminal gets status lines
//! plus live bars and spinners; a pipe, or `--no-progress`, gets the status
//! lines alone, so a slow command still says what it is doing without frames;
//! `--json` turns each line into an NDJSON event; `--quiet` silences it.
//! Outside Live mode the bars are hidden [`Progress`] values, so callers
//! never branch.

use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use clap::builder::styling::Style;

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
    } else if out.progress && std::io::stderr().is_terminal() && redraws() {
        Mode::Live
    } else {
        Mode::Lines
    };
    set(mode, !out.plain, out.color_on_stderr());
}

/// Whether the terminal erases a line on `\x1b[K`, which a live display
/// redraws with. A Windows console does once virtual terminal processing is
/// on, which this turns on for the console itself, so a redirected stdout
/// does not keep it off; a console too old for it fails, and the display
/// then falls back to status lines. Off Windows there is nothing to turn on.
fn redraws() -> bool {
    enable_ansi_support::enable_ansi_support().is_ok()
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
/// printer, which runs whatever the mode is.
pub fn event_line(event: &serde_json::Value) {
    relay(event);
}

/// Write `line` on stderr as it is: an event, or a line another dataseek
/// process wrote to its stderr, as `mcp` relays its tool calls'. A failed
/// write is dropped: stderr is where it would be reported.
pub fn relay(line: impl Display) {
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

/// `1 source`, `3 sources`: a count with its noun in agreement.
pub fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

/// A spinner for work of unknown length. Without a live terminal the message
/// becomes a stage line instead, so the wait is still announced.
pub fn spinner(msg: impl Into<String>) -> Progress {
    let msg = msg.into();
    if mode() != Mode::Live {
        stage(&msg);
        return Progress::hidden();
    }
    Progress::start(msg, None)
}

/// A bar over `len` known units; use a [`spinner`] when the total is unknown.
/// Hidden unless a terminal is watching; the stage line before it carries the
/// news everywhere else.
pub fn bar(len: u64, msg: impl Into<String>) -> Progress {
    if mode() != Mode::Live {
        return Progress::hidden();
    }
    Progress::start(msg.into(), Some(len))
}

/// How often a live spinner or bar is redrawn.
const TICK: Duration = Duration::from_millis(90);

/// A spinner or a bar on stderr, redrawn by a thread of its own. Clones share
/// one display, so workers can count on it; a hidden one draws nothing.
#[derive(Clone)]
pub struct Progress(Option<Arc<Live>>);

struct Live {
    msg: String,
    len: Option<u64>,
    pos: AtomicU64,
    started: Instant,
    done: AtomicBool,
    ticker: Mutex<Option<JoinHandle<()>>>,
}

impl Progress {
    pub fn hidden() -> Self {
        Self(None)
    }

    #[cfg(test)]
    fn is_hidden(&self) -> bool {
        self.0.is_none()
    }

    fn start(msg: String, len: Option<u64>) -> Self {
        let live = Arc::new(Live {
            msg,
            len,
            pos: AtomicU64::new(0),
            started: Instant::now(),
            done: AtomicBool::new(false),
            ticker: Mutex::new(None),
        });
        let watched = Arc::downgrade(&live);
        let ticker = std::thread::spawn(move || {
            let mut tick = 0_usize;
            while let Some(live) = watched.upgrade()
                && !live.done.load(Ordering::Relaxed)
            {
                live.draw(tick);
                drop(live);
                tick = tick.wrapping_add(1);
                std::thread::park_timeout(TICK);
            }
        });
        *live.ticker.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(ticker);
        Self(Some(live))
    }

    pub fn inc(&self, units: u64) {
        if let Some(live) = &self.0 {
            live.pos.fetch_add(units, Ordering::Relaxed);
        }
    }

    /// Stop drawing and wipe the line, at once rather than at the next tick.
    pub fn finish_and_clear(&self) {
        let Some(live) = &self.0 else { return };
        live.done.store(true, Ordering::Relaxed);
        let ticker =
            live.ticker.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(ticker) = ticker {
            ticker.thread().unpark();
            let _ = ticker.join();
        }
        clear_line();
    }
}

impl Live {
    fn draw(&self, tick: usize) {
        let width = terminal_size::terminal_size_of(std::io::stderr())
            .map_or(80, |(w, _)| usize::from(w.0));
        let line = frame(
            &self.msg,
            self.len,
            self.pos.load(Ordering::Relaxed),
            self.started.elapsed(),
            tick,
            width,
        );
        let _ = write!(anstream::stderr(), "\r{line}");
        erase_to_end();
    }
}

/// A display dropped without [`Progress::finish_and_clear`], as on an early
/// return, still leaves a clean line. One already cleared is left alone, since
/// the line under the cursor may by then be output.
impl Drop for Live {
    fn drop(&mut self) {
        if !self.done.load(Ordering::Relaxed) {
            clear_line();
        }
    }
}

fn clear_line() {
    let _ = write!(std::io::stderr(), "\r");
    erase_to_end();
}

/// Erase from the cursor to the end of the line. Written past anstream,
/// which strips every escape sequence when color is off, not only colors,
/// so `NO_COLOR` would leave each frame's tail and the last frame on screen.
/// A display draws only on a live terminal, which understands it.
fn erase_to_end() {
    let _ = write!(std::io::stderr(), "\x1b[K");
}

/// One frame, `  ⠹ msg 3s` or `  msg [███░░░] 2/4 3s`, cut to fit a terminal
/// `width` columns wide by shortening the message and, below a bar's width,
/// dropping the bar, so it does not wrap where the counts alone fit.
fn frame(
    msg: &str,
    len: Option<u64>,
    pos: u64,
    elapsed: Duration,
    tick: usize,
    width: usize,
) -> String {
    const BAR: usize = 30;
    let secs = elapsed.as_secs();
    let time = if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    };
    let (mark, suffix) = match len {
        None => {
            let ticks = if unicode() {
                "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
            } else {
                "|/-\\"
            };
            let at = tick.checked_rem(ticks.chars().count()).unwrap_or(0);
            let spin = ticks.chars().nth(at).unwrap_or(' ').to_string();
            (Some(spin), format!(" {time}"))
        }
        Some(len) => {
            let (full, empty) =
                if unicode() { ("█", "░") } else { ("#", "-") };
            let filled = pos
                .min(len)
                .saturating_mul(BAR as u64)
                .checked_div(len)
                .and_then(|f| usize::try_from(f).ok())
                .unwrap_or(0);
            let bar = full.repeat(filled)
                + &empty.repeat(BAR.saturating_sub(filled));
            let counts = format!(" {pos}/{len} {time}");
            // The indent, the brackets, four columns of message and the
            // column the cursor rests in.
            let bar_fits =
                BAR.saturating_add(10).saturating_add(counts.len()) <= width;
            let suffix = if bar_fits {
                format!(" [{}]{counts}", paint(palette::accent(), bar))
            } else {
                counts
            };
            (None, suffix)
        }
    };
    let indent = if mark.is_some() { 4 } else { 2 };
    let fixed = visible(&suffix).saturating_add(indent);
    let room = width.saturating_sub(1).saturating_sub(fixed);
    let msg = if msg.chars().count() <= room {
        msg.to_owned()
    } else if room > 3 {
        let cut: String = msg.chars().take(room.saturating_sub(3)).collect();
        cut + "..."
    } else {
        msg.chars().take(room).collect()
    };
    match mark {
        Some(spin) => {
            format!("  {} {msg}{suffix}", paint(palette::accent(), spin))
        }
        None => format!("  {msg}{suffix}"),
    }
}

/// Characters a string shows, escape sequences not counted.
fn visible(text: &str) -> usize {
    let mut in_escape = false;
    text.chars()
        .filter(|&c| {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                in_escape = !c.is_ascii_alphabetic();
                return false;
            }
            !in_escape
        })
        .count()
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

    #[test]
    fn frames_show_progress_and_fit_the_terminal() {
        let _guard = locked();
        set(Mode::Live, false, false);
        let secs = Duration::from_secs;
        assert_eq!(
            frame("scanning", None, 0, secs(3), 1, 80),
            "  / scanning 3s"
        );
        assert_eq!(
            frame("sources", Some(4), 2, secs(75), 0, 80),
            "  sources [###############---------------] 2/4 1m 15s"
        );
        assert_eq!(
            frame("sources", Some(0), 0, secs(0), 0, 80),
            "  sources [------------------------------] 0/0 0s"
        );
        assert_eq!(
            frame(
                "a very long message about catalogs",
                None,
                0,
                secs(0),
                4,
                20
            ),
            "  | a very lo... 0s"
        );
        assert_eq!(frame("scanning", None, 0, secs(3), 1, 10), "  / sc 3s");
        assert_eq!(
            frame("sources", Some(4), 2, secs(75), 0, 30),
            "  sources 2/4 1m 15s"
        );
        let plain = frame("x", Some(1), 1, secs(0), 0, 80);
        set(Mode::Live, false, true);
        let colored = frame("x", Some(1), 1, secs(0), 0, 80);
        assert_ne!(colored, plain);
        assert_eq!(visible(&colored), plain.chars().count());
    }

    #[test]
    fn a_live_display_stops_at_once_and_with_its_last_handle() {
        let _guard = locked();
        let started = Instant::now();
        let finished = Progress::start("x".to_owned(), Some(2));
        finished.inc(1);
        finished.finish_and_clear();
        assert!(started.elapsed() < Duration::from_secs(1));
        let dropped = Progress::start("y".to_owned(), None);
        let watched = dropped.0.as_ref().map(Arc::downgrade).unwrap();
        drop(dropped);
        while watched.upgrade().is_some() {
            assert!(started.elapsed() < Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn counts_agree_with_their_noun() {
        assert_eq!(count(0, "source"), "0 sources");
        assert_eq!(count(1, "source"), "1 source");
        assert_eq!(count(2, "catalog"), "2 catalogs");
    }
}
