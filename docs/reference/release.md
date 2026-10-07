# Release

Versioning is automatic: release-plz bumps `Cargo.toml`, writes [`../CHANGELOG.md`](../CHANGELOG.md), tags `vX.Y.Z` and cuts the GitHub release. Why it is built this way is [ADR 0001](../adr/0001-release-plz-owns-the-version.md); what moves the number is [ADR 0003](../adr/0003-the-cli-surface-sets-the-version.md).

## Conventional commits

Write [conventional commits](https://www.conventionalcommits.org). The prefix decides both the changelog group and whether a release happens at all:

| prefix | changelog group | cuts a release |
|---|---|---|
| `feat:` | Added | yes, minor |
| `fix:` | Fixed | yes, patch |
| `perf:` | Performance | yes, patch |
| `refactor:` | Changed | yes, patch |
| `doc:` | Docs | no |
| `build:`, `chore:` | Other | no |
| `chore(release):`, `test:`, `ci:` | skipped | no |
| anything else | Uncategorized | no |

Uncategorized is the changelog telling you a subject was not conventional. Add `!` after the prefix for a breaking change; its changelog line then starts with "Breaking:".

## The loop

On every push to `main`, the `release-pr` job opens or updates a PR that bumps the version and writes the changelog section from the commits since the last tag, then queues it for auto-merge. CI runs on that PR like on any other; once the required `linux` check is green, GitHub squash-merges it and deletes the branch, and the `release` job on that merge commit tags it and cuts the GitHub release, whose body is the changelog section. Publishing that GitHub release starts `.github/workflows/release.yml`, which builds the wheels and uploads them to PyPI (below). Nobody clicks anything between a `feat:` landing and its release.

Nothing earlier than v0.3.0 exists: release-plz diffs against the newest `v*` tag, and that baseline was tagged by hand.

## The config

`.github/release-plz.toml`, passed through the action's `config` input in both jobs and `--config` locally:

- `git_release_enable = true`, `git_release_type = "auto"`, `git_release_body = "{{ changelog }}"`: every tag gets a GitHub release carrying that version's section.
- `release_commits`: release-plz decides whether to release from changed files, not from changelog groups, so without it a `ci:` merge alone opens a release with an empty section. That is how v0.3.1 happened.
- `features_always_increment_minor = true`: see ADR 0003.
- `semver_check = false`: a binary has no public API for cargo-semver-checks to compare.
- `release_always = false`: tag only when the release PR merges.
- `pr_name` and `pr_body` replace the defaults, which carry a robot emoji and a generated-with footer.
- `[[package]] changelog_path = "docs/CHANGELOG.md"`: relative to the root `Cargo.toml`. It cannot be set under `[workspace]`.
- `commit_parsers`: git-cliff takes the first match, so `^chore\(release\)` sits ahead of `^chore`. The trailing `.*` catch-all exists because without it a subject matching no rule is dropped from the changelog silently. `test` and `ci` are skipped because a user of the binary cannot observe them, and so are merge commits, whose branches' own commits already carry the changes; `build` stays because it carries MSRV and toolchain moves.

Section headings read `## <version> - <YYYY-MM-DD>`.

## The release token

Both jobs mint a one-hour token from the `lmarkmann-release` GitHub App with Contents and Pull requests write; the job's own `GITHUB_TOKEN` stays read-only. The repository holds `RELEASE_APP_CLIENT_ID` (an Actions variable) and `RELEASE_APP_PRIVATE_KEY` (a secret), set from 1Password (`GITHUB_RELEASE_APP` in the Developer vault) by the ci-vcm skill's `app_secrets.sh`, and the App is installed on the repo. The key does not expire.

Auto-merge needs squash merges titled by the PR, auto-merge allowed, branches deleted on merge, and a ruleset requiring the `linux` check.

## Preview the next release

```sh
just release-preview
```

No release-plz subcommand has a dry-run flag, so the recipe runs the real `release-plz update`, prints the diff and restores the tree on every exit path. It refuses to start on a dirty tree, untracked files included, because the restore ends in `git clean`.

## Publishing to crates.io

`cargo install dataseek` installs `dataseek` and `dsk`. The package holds only what building needs (`cargo package --list`); the `internals` feature is for the benches and unstable.

The `release` job publishes through crates.io trusted publishing (`lmarkmann/dataseek`, workflow `release-plz.yml`, environment `crates-io`); no registry token is stored.

## Publishing to PyPI

`uvx dataseek` runs a wheel holding both binaries, built by maturin with the version from `Cargo.toml`. `release.yml` builds the wheels on each GitHub release and uploads them with `uv publish` through trusted publishing ([ADR 0017](../adr/0017-release-builds-on-a-platform-matrix.md)). The same jobs attach `dataseek-<target>.tar.gz` (`.zip` on Windows) with a `.sha256` to the release, which `cargo binstall dataseek` downloads.

Before the first release, add a pending publisher at <https://pypi.org/manage/account/publishing/>: project `dataseek`, `lmarkmann/dataseek`, workflow `release.yml`, environment `pypi`.

`dsk` is also an unrelated PyPI project; installing both into one environment makes their scripts collide.
