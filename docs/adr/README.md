# Architecture decision records

A decision that constrains future work lives here, once, and is cited from wherever it is load-bearing. Config files and workflow headers carry a one-line pointer at most.

## What belongs here, and what does not

| the fact | its home |
|---|---|
| a decision that would be expensive to reverse or tempting to re-litigate | **here**, one numbered file each |
| something considered and left out, with the date | [`rejected.md`](rejected.md) |
| something parked as *not yet*, with the trigger that revisits it | [`watchlist.md`](watchlist.md) |
| how to run, configure or extend something | [`../reference/`](../reference/) |
| what reads across several dataset sources | [`../synthesis/`](../synthesis/) |
| what shipped | [`../CHANGELOG.md`](../CHANGELOG.md) |
| an invariant a caller must not break | the source comment, in one line |

## Format

`NNNN-kebab-title.md` with four sections: **Context** (what forced the decision), **Decision** (present tense), **Consequences** (what it costs, and what breaks if it is reverted), **Evidence** (the measurement or incident that settled it). Under about fifty lines. Superseding an ADR means writing a new one and marking both, never editing the old one into agreement.

## The record

| # | decision | status |
|---|---|---|
| [0001](0001-release-plz-owns-the-version.md) | release-plz owns the version, the tag and the changelog | accepted |
| [0002](0002-one-linux-ci-job.md) | CI is one Linux job that cross-checks Windows and macOS | accepted |
| [0003](0003-the-cli-surface-sets-the-version.md) | The CLI surface sets the version | accepted |
