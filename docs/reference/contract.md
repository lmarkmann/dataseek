# Quality contract

The code, not the README, enforces this contract. The lints in `Cargo.toml` exist for exactly that reason: a rule that only fires in CI is found too late to be useful.

## Version

The version lives only in `Cargo.toml`. Clap reads it via `#[command(version)]`, so `--version` prints `name x.y.z` and nothing else. Never hardcode a version string anywhere else.

## Flags

- `-h/--help`, `-V/--version`
- Global `-q/--quiet`, `-v/--verbose` (repeatable)
- `--json`, `--jq EXPR`, `--color=auto|always|never`, `--no-color`, `--plain`, `--no-progress`
- `--cache-dir DIR` and `--connect-timeout SECS`, each with a `DATASEEK_*` variable

`search` reads `-x`, `--per-source`, `-n` and `--timeout` from `DATASEEK_EXCLUDE`, `DATASEEK_PER_SOURCE`, `DATASEEK_LIMIT` and `DATASEEK_TIMEOUT` too. A flag beats its variable, which beats the default; `dataseek help environment` lists them all. A source marked `opt-in` in `dataseek sources` runs only when `-s` names it ([ADR 0013](../adr/0013-opt-in-sources.md)).

`-v` is verbose, never version. `--color` follows the ecosystem standard used by git, ripgrep, and fd:

- `auto` colors only on a terminal and honors `NO_COLOR`.
- `never` strips color.
- `always` forces color even through a pipe, for `| less -R`.

`--no-color` and `--plain` are shorthands for `--color=never`. Because clap prints its own help and usage errors before the flags are resolved, `src/lib.rs` scans the arguments for these three first and hands clap the result, so `dsk --no-color nonsense` is as colorless as `dsk --no-color sources`.

`-h` lists the flags people type daily and at most three examples; `--help` adds the rest (`--no-color`, `--no-progress`, `--cache-dir`, `--connect-timeout`), the exit codes, the help topics and the issues address.

Secrets never arrive as flags, which leak into shell history and process listings: API keys come from the environment or `credentials.toml` ([`keys-and-cache.md`](keys-and-cache.md), ADR 0009).

## Help surfaces

- A bare `dataseek` prints the overview on **stdout** and exits `0`: the name and version, the commands in three small groups (search, upkeep, shell), and a footer pointing at `-h`. It is orientation, so `dsk | head` shows it. `src/help.rs` holds the groups; a unit test fails when a command is missing from them.
- `dataseek help <command>` is that command's `--help`. `dataseek help environment` lists the variables that change what it does, `dataseek help exit-codes` every code it returns.
- `dataseek help --json` describes the whole surface as data for scripts and agents: commands, flags with their type (`boolean`, `integer`, `string` or `count`), value names, possible values, defaults and variables, and the exit codes, all generated from the clap definition. A bare `dataseek --json` prints the same object. `dataseek help <command> --json` prints that command's entry of it: its own flags, with the global ones listed once, on the root.

## stdout is data, stderr is status

Every byte of stdout goes through a handle from `src/output.rs`, including the two commands whose output is raw data, `completion` and `man`. Narration, spinners, and progress go through `src/ui.rs` on stderr, in one of four modes:

| who reads stderr | what it gets |
|---|---|
| a terminal | status lines plus live bars and spinners with elapsed time |
| a pipe, or `--no-progress` | the same status lines, plain, and no frames |
| `--json` | each status line, and each `-v` note, as an NDJSON event tagged `dataseek-events/1` |
| `--quiet` | nothing but the error |

So a piped search still announces itself at once and sums up at the end, and no spinner frame can reach a log. What must never happen is a frame on stdout, and that is guaranteed by the layer only ever writing to stderr. `doctor` does not narrate: its checks take milliseconds, so it prints only its report.

`search` prints its summary after the results, so the last line says how many matched, how many sources answered and how to see more. An empty result is a stderr line, never text on stdout.

This is why `Cargo.toml` denies `print_stdout` and `print_stderr`: a stray `println!` is a contract violation, not a style preference.

## JSON

Every `--json` output is one object with a `schema` tag naming its shape and version (`dataseek-search/1`, `dataseek-sources/1`, `dataseek-doctor/1`, ...). A key that changes meaning or disappears bumps the tag; a new key does not. On a terminal the JSON is indented, in a pipe it is one line. `--jq EXPR` runs a jq expression over the same object (strings come out raw, like `jq -r`), so a script needs no second process; a broken expression fails before any work starts. `completion` and `man` print their script and page whatever the flag. `tests/snapshots/` freezes the search, sources, doctor, cache info, exit-codes and error-event shapes.

## MCP mode

`dataseek mcp` serves `search`, `sources` and `inspect` to MCP clients over stdio ([ADR 0014](../adr/0014-mcp-server-over-stdio.md)). A client registers it as command `dsk` with arguments `["mcp"]`.

- **Protocol.** Requests naming revision `2026-07-28` in `_meta` are served statelessly, with `server/discover`; clients that open with `initialize` get revision `2025-11-25` or `2025-06-18`. A request naming any other revision gets `-32022` with the supported list.
- **stdout** carries one JSON-RPC message per line and nothing else. The server's status line and its tool calls' narration, as `dataseek-events/1` lines, go to stderr; `-q` and `-v` on `dataseek mcp` reach every call.
- **Tools.** Each tool's input schema is its command's `help <command> --json` entry: one property per flag, named by its long form, typed, with its possible values and default; `search`'s words are one `query` string. A call runs `dataseek <command> --json` with those flags, and the result carries the object that prints as `structuredContent` and again as text.
- **Failures.** A failed call is a result with `isError: true`: the text is the `Error:`, `Cause:` and `Try:` lines the command line prints, and `structuredContent` the error event. A bad argument is such a failure too, since the model can fix it. A malformed message, an unknown method or tool, or a server fault is a JSON-RPC error: `-32700`, `-32600`, `-32601`, `-32602`, `-32603`.
- **Deadline.** Every search runs with `--timeout`, the argument or its default of 20 seconds; `timeout: 0` is refused.
- **Keys** come from the environment and `credentials.toml`, as everywhere ([ADR 0009](../adr/0009-keys-and-contact-address.md)); no tool takes one as an argument.
- **Lifetime.** Closing stdin ends the server at once, and any call still running is killed; `notifications/cancelled` kills one call, which then gets no answer.

## SIGPIPE

`src/lib.rs` resets `SIGPIPE` to the Unix default so `tool | head` exits quietly instead of panicking.

Both the call and the `signal-hook` dependency are gated to `cfg(unix)`, because signals are a Unix concept: `signal_hook::consts::SIGPIPE` does not exist on Windows, where the crate re-exports only `SIGABRT`, `SIGFPE`, `SIGILL`, `SIGINT`, `SIGSEGV`, and `SIGTERM`. An unconditional import there is not a runtime problem, it is a build failure, and it went unnoticed until Windows joined the CI matrix. Nothing is lost on Windows: a closed pipe surfaces as an ordinary write error, which the broken-pipe arm of `report()` already turns into a quiet success.

The dependency is scoped under `[target.'cfg(unix)'.dependencies]` rather than imported and `cfg`'d away at the call site, so Windows does not build `signal-hook` or `libc` at all and `cargo-deny` stops reasoning about crates that platform never compiles.

## Errors

Failures are typed with `thiserror` and printed on stderr as:

```text
Error: <what went wrong>
  Cause: <underlying cause>
  Try:   <recovery hint>
```

The hint is baked into the thiserror message after a `\n  Try:` marker; `report()` splits it off and prints the `Cause:` chain between the two, so the order is the problem, why, then what to do. A cause is printed once, from the chain; a message that also interpolates its own `{source}` prints it twice. An error with no hint was not anticipated, so its `Try:` line is the issues address. Under `--json` the same three parts are one `error` event on stderr, and clap's usage errors take that shape too.

Expected failures never show a stack trace. `Cargo.toml` denies every route a panic takes into production code, because a panic is a bug and never a way to exit: `unwrap_used`, `expect_used`, `panic`, `panic_in_result_fn`, `unreachable`, `unimplemented`, `todo`, and the two that are easy to overlook because they carry no macro name, `indexing_slicing` and `arithmetic_side_effects`. Exit codes are returned as `ExitCode` from `main` so destructors still run.

## Exit codes

- `0`: success
- `1`: failure
- `2`: usage error (clap)
- `130`: interrupted by Ctrl-C (death by SIGINT, which the shell reports as 130)

For `search`, success means at least one source answered, even with no results: an empty answer is a result (ADR 0007). `1` means every attempted source failed, or none could run: each lacks its key, each is resting after an outage in the last ten minutes, or `-s`, `-c` and `-x` left none. When every attempted source was unreachable (no DNS answer, refused, a TLS failure; a timeout does not count), the error says the machine looks offline and points at `--offline`; under `--offline` with nothing cached, it says so instead. `--offline` and `-s` ask resting sources anyway. A source that fails while others answer is a warning on stderr, never a failure. `cache warm` is the exception that fails partway: any catalog that did not download makes the run exit `1`, after every success is printed and every failure named, on stderr and again in the error, so `-q` still says which.

`src/help.rs` holds the list `help exit-codes` prints, and `tests/cli.rs` asserts every code on it.

## Completion and the man page

`dataseek completion fish` (or `bash`, `zsh`, ...) prints a completion script to stdout. `dataseek man` prints the man page, in roff, to the same place:

```sh
dataseek man | man -l -
dataseek man > ~/.local/share/man/man1/dataseek.1
```

Both are data, so both go to stdout, and both are derived from the clap definition rather than maintained by hand. The man page is rendered on demand by `clap_mangen` instead of at build time by a build script, so it cannot drift from the flags. The tests assert the page carries the manifest version, renders without errors and names every command, and that fish parses the completion script.

## Color

Every color the tool emits is defined once in `src/palette.rs`:

```rust
const ACCENT: Color = ansi(AnsiColor::Blue);     // headers, the signature hue
const SUCCESS: Color = ansi(AnsiColor::Green);   // values, "it worked"
const WARNING: Color = ansi(AnsiColor::Yellow);  // soft alerts, placeholders
const DANGER: Color = ansi(AnsiColor::Red);      // errors, destructive prompts
const MUTED: Color = ansi(AnsiColor::BrightBlack); // secondary detail
```

Roles are semantic (`accent`, `success`, `danger`), not literal, so swapping a color is one edit. Use a named ANSI color for portability, or 24-bit truecolor with `rgb(0x7a, 0xa2, 0xf7)`.

The same roles drive clap help styling, stdout text, and the stderr progress accent. `anstream` strips escapes whenever the stream is not a terminal, so call sites style unconditionally and never branch on color. On a color terminal, result URLs are OSC 8 hyperlinks and long titles and descriptions are cut to the width (`COLUMNS` wins) with an ellipsis; in a pipe every line stays whole.

indicatif parses its color token with `console::Style::from_dotted_str`, which understands named colors, `#rrggbb` truecolor, and a bare 256-color index, so `palette::accent_token()` renders whichever shape the accent takes exactly. Spinners and bars match help and stdout rather than approximating them.

## Platform directories

`src/paths.rs` resolves config, cache, and state separately through `etcetera`, using XDG-style directories. The tool never writes dotfiles directly into `$HOME`. `--cache-dir` (or `DATASEEK_CACHE_DIR`) moves the cache alone.

## Atomic writes

`src/fs.rs` writes to a temp file in the target directory and then renames, so a crash mid-write cannot corrupt the original file. `cache clear --dry-run` and `cache warm --dry-run` report what the real run would do, in the same shape, and touch nothing.

## Extending the CLI

Add a subcommand by adding a variant to `Command` in `src/cli.rs`, a match arm in `run()` in `src/lib.rs`, its name in `COMMANDS` and a line in `GROUPS` in `src/help.rs`. Keep one domain per tool; prefer support subcommands (`doctor`, `cache`) over piling flags onto the root. Add a test in `tests/cli.rs` for every behavior you add, and give a command that takes arguments people get wrong at most three examples, and add its `-h` to the list `every_example_in_help_and_readme_parses` reads.

The unit test at the bottom of `src/cli.rs` calls clap's `debug_assert()` on the built command, which catches a malformed definition that still compiles: a short flag used twice, a `default_value` outside the possible values, an arg that conflicts with itself. Those otherwise panic the first time a user reaches them, inside clap, where the `panic = "deny"` lint cannot see them. A proptest beside it feeds arbitrary argv to the parser for the same reason.
