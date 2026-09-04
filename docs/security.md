# Security

The template runs two supply-chain gates in CI and locally through `just audit`.

## cargo-deny

`cargo-deny` is configured in `deny.toml`.

- `advisories`: RUSTSEC advisories are errors by default. Add an ID with a note and a date only when there is no fix and the code path is unreachable.
- `licenses`: only permissive licenses are allowed. The allowlist is deliberately wider than the current dependency tree so a clone adding a dependency does not fail on a license that was always going to be fine.
- `bans`: duplicate versions are warned, not failed. Wildcards in dependency requirements are denied.
- `sources`: only crates.io is allowed.

The workspace member itself has no `license` field because the crate is `publish = false`; `licenses.private.ignore` tells cargo-deny not to flag it.

Run it with `just deny`.

## zizmor

`zizmor` audits the GitHub Actions workflows themselves. It is configured in `.github/zizmor.yml`.

- `unpinned-uses`: two policies, because the two workflows carry different risk. `ci.yml` runs on version tags. Every action there comes from a trusted publisher (`actions/`, `dtolnay/`, `Swatinem/`, `taiki-e/`, `crate-ci/`, `EmbarkStudios/`, `zizmorcore/`), and that workflow is read-only, holds no secret and ships nothing, so the worst a rewritten tag reaches is a build nobody consumes. Keep that list honest when you add an action; a trust argument with a name missing is not one. `release-plz.yml` is hash-pinned instead, because it is the one workflow holding a token and the one running with `contents: write`, where a rewritten tag reaches something that can push to `main` and cut releases. `"release-plz/*": hash-pin` enforces the half that can be expressed: zizmor keys policies by action rather than by workflow, and `actions/checkout` appears in both files at different strictness, so no pattern can hold it to the stricter rule in one file alone.

Hash pins are written by [`pinact`](https://github.com/suzuki-shunsuke/pinact), which also writes the trailing `# v7.0.1` version comment. That comment is load-bearing rather than decorative: `just actions-outdated` reads it to compare the pin against upstream, so a SHA without one silently drops out of the freshness report, and the recipe now fails loudly instead. Re-pin with:

```sh
pinact run --update --exclude 'dtolnay/rust-toolchain' .github/workflows/release-plz.yml
```

The exclusion is not a preference. `dtolnay/rust-toolchain@stable` names a toolchain rather than a version, and pinact refuses it on its own with `action can't be pinned`.
There are no rule exemptions. `artipacked` was once ignored for `release-plz.yml`, on the assumption that release-plz needed its checkout credentials to push; it pushes through the GitHub API instead, so every checkout in the repo now sets `persist-credentials: false` and the ignore is gone. The two cases that would need the credentials back are signed tags and a step that runs `git push` after release-plz.

The CI job in `.github/workflows/ci.yml` runs `zizmorcore/zizmor-action` with `advanced-security: false`, which prints findings to the log and fails the job on them. The action's default instead uploads a SARIF report to the Security tab, which is the richer option but a poor default for a template: it needs the repository to be public or to pay for Advanced Security, and in that mode the action deliberately does not fail on findings, since GitHub expects a merge-protection ruleset to be the blocking signal. A job that goes green while findings sit in a tab is the same failure as the MSRV gate that checks the wrong toolchain. A clone that is public and wants the Security tab can drop the input and grant `security-events: write`, plus `actions: read` and `contents: read` if it is private with Advanced Security.

Both sides run the same audits: the action's `online-audits` input defaults to `true`, the opposite of the CLI default, so CI sets it to `false` to match `just zizmor`. Turning it on in both places is the other consistent choice, and costs a token locally.

Versions can still differ. `just zizmor` runs whichever binary is installed locally, while the action's `version` input defaults to `latest`, so CI downloads the newest release at run time rather than one bundled with the action's own tag. This is a real gap and not a theoretical one: the local binary sat at 1.29.0 while the action ran 1.30.0. If findings differ, compare `zizmor --version` against what the CI log reports, and pin `version:` in the workflow if you need them locked together.

Run it with `just zizmor`.

## Workflow permissions

Workflows use the smallest permissions they can:

- `ci.yml` sets `permissions: contents: read` at workflow level.
- The `zizmor` job restates `contents: read` and adds nothing, because `advanced-security: false` means it never writes to the Security tab.
- `release-plz.yml` sets `permissions: {}` at workflow level and grants `contents: write` and `pull-requests: write` only to the release-plz job.

Every checkout sets `persist-credentials: false`, including the two in `release-plz.yml`.
