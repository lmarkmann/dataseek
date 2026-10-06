# Quality contract

The template is organized so the code, not the README, enforces the contract. The lints in `Cargo.toml` exist for exactly that reason: a rule that only fires in CI is found too late to be useful.

## Version

The version lives only in `Cargo.toml`. Clap reads it via `#[command(version)]`, so `--version` prints `name x.y.z` and nothing else. Never hardcode a version string anywhere else.

## Flags

- `-h/--help`, `-V/--version`
- Global `-q/--quiet`, `-v/--verbose` (repeatable)
- `--json`, `--color=auto|always|never`, `--no-color`, `--plain`

`-v` is verbose, never version. `--color` follows the ecosystem standard used by git, ripgrep, and fd:

- `auto` colors only on a terminal and honors `NO_COLOR`.
- `never` strips color.
- `always` forces color even through a pipe, for `| less -R`.

`--no-color` and `--plain` are shorthands for `--color=never`.

## Conventions for clones

None of the following is implemented here, because no command in the template needs it. They are the rules to follow when your clone grows one, collected so the decision is already made when you get there.

- Name a **dry run** flag `-n, --dry-run`. It must describe the actions it would take without changing state.
- Prompt only when `stdin().is_terminal()` is true, and provide `--no-input` for automation. If a required answer is missing under `--no-input`, fail with a recovery hint instead of prompting or hanging.
- Never accept **secrets** through command-line flags, which leak into shell history and process listings. Read them from stdin or a file instead.
- Keep color at `auto` by default. It must honor both `NO_COLOR` and `TERM=dumb`; `--color=always` is the explicit override.

## stdout is data, stderr is status

Every byte of stdout goes through a handle from `src/output.rs`, including the two commands whose output is raw data, `completion` and `man`. Narration, spinners, and progress go through `src/ui.rs` on stderr.

The UI layer no-ops unless **stderr** is a terminal, and whenever `--json` or `--quiet` is set. Keying it on stderr rather than stdout is the point rather than an implementation detail: `tool doctor | wc -l` run by a person should still narrate, because someone is watching even though the data is piped. What must never happen is a spinner frame reaching stdout, and that is guaranteed by the layer only ever writing to stderr, not by guessing whether stdout is redirected.

This is why `Cargo.toml` denies `print_stdout` and `print_stderr`: a stray `println!` is a contract violation, not a style preference.

## SIGPIPE

`main.rs` resets `SIGPIPE` to the Unix default so `tool | head` exits quietly instead of panicking.

Both the call and the `signal-hook` dependency are gated to `cfg(unix)`, because signals are a Unix concept: `signal_hook::consts::SIGPIPE` does not exist on Windows, where the crate re-exports only `SIGABRT`, `SIGFPE`, `SIGILL`, `SIGINT`, `SIGSEGV`, and `SIGTERM`. An unconditional import there is not a runtime problem, it is a build failure, and it went unnoticed until Windows joined the CI matrix. Nothing is lost on Windows: a closed pipe surfaces as an ordinary write error, which the broken-pipe arm of `report()` already turns into a quiet success.

The dependency is scoped under `[target.'cfg(unix)'.dependencies]` rather than imported and `cfg`'d away at the call site, so Windows does not build `signal-hook` or `libc` at all and `cargo-deny` stops reasoning about crates that platform never compiles.

## Errors

Failures are typed with `thiserror` and printed on stderr as:

```text
Error: <what went wrong>
  Try:   <recovery hint>
  Cause: <underlying cause>
```

The hint is baked into the thiserror message, so it is part of the `Error:` line's own text and necessarily precedes the `Cause:` lines that `report()` appends from the error chain. That order is also the one worth wanting: the problem, then the thing to do about it, then the low-level detail last. A cause is printed once, from the chain; a message that also interpolates its own `{source}` prints it twice.

Expected failures never show a stack trace. `Cargo.toml` denies every route a panic takes into production code, because a panic is a bug and never a way to exit: `unwrap_used`, `expect_used`, `panic`, `panic_in_result_fn`, `unreachable`, `unimplemented`, `todo`, and the two that are easy to overlook because they carry no macro name, `indexing_slicing` and `arithmetic_side_effects`. Exit codes are returned as `ExitCode` from `main` so destructors still run.

## Exit codes

- `0`: success
- `1`: failure
- `2`: usage error (clap)

Naked invocation prints help and exits `0`, because asking for help is not a usage error. clap sends that help to **stderr** and leaves stdout empty, which is the right side of the split: a bare invocation in a pipeline hands the consumer nothing rather than a help page. `--help` asked for explicitly is a result, so it goes to stdout.

## Completion and the man page

`dataseek completion fish` (or `bash`, `zsh`, ...) prints a completion script to stdout. `dataseek man` prints the man page, in roff, to the same place:

```sh
dataseek man | man -l -
dataseek man > ~/.local/share/man/man1/dataseek.1
```

Both are data, so both go to stdout, and both are derived from the clap definition rather than maintained by hand. The man page is rendered on demand by `clap_mangen` instead of at build time by a build script: it then cannot drift from the flags, and the clone gets no `build.rs` and no `OUT_DIR` lookup to understand. The test in `tests/cli.rs` asserts the page carries the manifest version, so it is held to the same single-source rule as `--version`.

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

The same roles drive clap help styling, stdout text, and the stderr progress accent. `anstream` strips escapes whenever the stream is not a terminal, so call sites style unconditionally and never branch on color.

This holds for the progress layer too. indicatif parses its color token with `console::Style::from_dotted_str`, which understands named colors, `#rrggbb` truecolor, and a bare 256-color index, so `palette::accent_token()` renders whichever shape the accent takes exactly. Spinners and bars match help and stdout rather than approximating them.

## Platform directories

`src/paths.rs` resolves config, cache, and state separately through `etcetera`, using XDG-style directories. The tool never writes dotfiles directly into `$HOME`.

## Atomic writes

`src/fs.rs` writes to a temp file in the target directory and then renames, so a crash mid-write cannot corrupt the original file.

## Extending the CLI

Add a subcommand by adding a variant to `Command` in `src/cli.rs` and a match arm in `src/main.rs`. Keep one domain per tool; prefer support subcommands (`doctor`, `info`, `logs`) over piling flags onto the root. Add a test in `tests/cli.rs` for every behavior you add.

The unit test at the bottom of `src/cli.rs` calls clap's `debug_assert()` on the built command, which catches a malformed definition that still compiles: a short flag used twice, a `default_value` outside the possible values, an arg that conflicts with itself. Those otherwise panic the first time a user reaches them, inside clap, where the `panic = "deny"` lint cannot see them. It lives in `src/cli.rs` and not in `tests/cli.rs` because this is a binary crate, so an integration test can run the binary but never build the `Command`.
