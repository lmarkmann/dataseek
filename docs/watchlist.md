# Watch list

Things that are not rejected and not adopted: each one is a clear *not yet*, with the trigger that would make it worth revisiting. This is the companion to [`rejected.md`](rejected.md). A rejected entry needs its reason to stop being true; a watch entry needs its trigger to fire. When one does, the entry leaves this file and becomes either a commit or a dated entry in `rejected.md`.

Keep entries small: the item, why it is parked, and the trigger. If you cannot name the trigger, the item does not belong here.

## clap_complete dynamic completions

Completing *values* (branch names, file arguments from the tool's own index) instead of just flags, by having the shell call back into the binary. Parked because the mechanism lives behind clap_complete's `unstable-dynamic` feature and upstream warns that shell code and binary can drift apart (see the full reasoning in [`rejected.md`](rejected.md)).

Trigger: the feature drops the `unstable-` prefix in a clap_complete release. Check `cargo info clap_complete` and its CHANGELOG.

## insta-cmd

From the insta authors: snapshots a spawned process's stdout, stderr, and exit code in a single `.snap`, collapsing the assert_cmd + insta pairing in `tests/cli.rs` into one tool. Parked because the current pairing works and rewriting a working test harness for ergonomics alone churns every snapshot.

Trigger: the next time the test harness is touched anyway (a new kind of assertion, a snapshot format annoyance), evaluate `insta-cmd` as part of that change.

## clap 5

A `5.0` milestone exists in the clap repo with no date. The template's styling (`palette::help()`), error-kind matching in `main.rs`, and derive attributes all touch APIs that a major release tends to move. Parked because there is nothing to migrate to yet.

Trigger: a clap 5.0 release. Read the migration guide before bumping; expect the `styles` and `error` modules and possibly the `unstable-dynamic` completion story above to land in the same move.

## terminal-colorsaurus

Queries the terminal for a dark or light background so defaults stay readable; bat and delta use it. Parked because the palette is five semantic constants chosen to work on both, and a runtime query adds a fallback path (what happens when the terminal does not answer) for a problem nobody has reported.

Trigger: the palette gains a color that is genuinely unreadable on one background, or a user reports one.
