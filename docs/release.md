# Release

Versioning is automatic through `release-plz`.

## Conventional commits

Write [conventional commits](https://www.conventionalcommits.org) so release-plz can group them:

- `feat:` -> Added
- `fix:` -> Fixed
- `perf:` -> Performance
- `refactor:` -> Changed
- `doc:` -> Docs
- `build:` -> Other
- `chore:` -> Other
- `chore(release):`, `test:` and `ci:` -> skipped
- anything else -> **Uncategorized**, which is the changelog telling you the subject was not conventional

## How release-plz works

On every push to `main`, `release-plz` opens a PR that bumps the version in `Cargo.toml` and rewrites `CHANGELOG.md` from the conventional commits since the last tag. Merging that PR tags the release.

The workflow runs those two commands as separate jobs, which is the layout upstream supports; a single job running both is documented as "not recommended". The split matters for more than support, because only `release-pr` carries a `concurrency` group. GitHub cancels an existing *pending* run in a concurrency group as soon as another queues, whatever `cancel-in-progress` says, so a grouped `release` job could have its run discarded while waiting behind an earlier push. With `release_always = false` the merge of the release PR is the one commit that cuts a tag, and losing that run loses the tag with no failure anywhere. Serializing `release-pr` is still right: two runs writing the same release branch race, and a superseded one costs nothing because the next push redoes it.

`release-plz.toml` configures this:

- `git_only = true`: the crate is `publish = false`, so the previous version comes from git tags rather than crates.io.
- `publish = false`: nothing is published.
- `git_release_enable = false`: a GitHub release would only mirror the tag; there are no binaries to attach.
- `semver_check = false`: a binary exposes no public API, so cargo-semver-checks has nothing to compare.
- `release_always = false`: tag only when the release PR merges, not on every push to main.

## The first release

Release-plz anchors itself to git tags: with no `v*` tag, it treats the manifest version as an initial release and has nothing to do, so the first tag has to come from somewhere. This repo is tagged `v0.3.0` at its release commit, so the automation is anchored; a clone created from a template starts with a single commit and no tags, so its first tag has to come from here:

```sh
git tag v0.3.0 && git push --tags
```

after which everything is automatic.

## Required secret

The workflow needs a repository secret named `RELEASE_PLZ_TOKEN`. It must be a fine-grained PAT with `contents: write` and `pull-requests: write`. The default `GITHUB_TOKEN` is not enough, because pushes made with it do not trigger the CI workflow on the release PR.

Until that secret exists the workflow is inert: both steps are guarded on it, so the job skips and reports green rather than failing. Releases are opt-in, and a fresh clone should not get a red X for a feature it has not set up yet. Create the secret with `gh secret set RELEASE_PLZ_TOKEN` when you want releases running.

## Preview the next release

```sh
just release-preview
```

No release-plz subcommand has a dry-run flag, so the recipe runs the real `release-plz update`, prints the diff, and restores the tree afterwards. It refuses to start unless the tree is clean, untracked files included, because the restore ends in a `git clean` and that needs a known starting point.

## Making a clone publishable

To turn a clone into something publishable:

1. Drop `publish = false` from `Cargo.toml`, and add `license`.
2. Drop `publish = false` and `git_only = true` from `release-plz.toml`. The first is what actually stops the publish; the second tells release-plz to read the previous version from git tags instead of from the registry, which is only correct while nothing is published.
3. Add `CARGO_REGISTRY_TOKEN` to the `release` job in `.github/workflows/release-plz.yml`. It has never been there, so this is a new secret rather than one to restore.
4. Consider `git_release_enable = true` and `semver_check` at the same time; both were turned off for reasons that stop applying once you ship. See `rejected.md`.
