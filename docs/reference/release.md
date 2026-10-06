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

On every push to `main`, `release-plz` opens a PR that bumps the version in `Cargo.toml` and rewrites `CHANGELOG.md` from the conventional commits since the last tag, and queues it for auto-merge. CI runs on that PR like on any other; once the required `linux` check is green GitHub merges it and deletes the branch, and that merge commit's run tags `vX.Y.Z` and cuts the GitHub release. Nobody clicks anything between a `feat:` or `fix:` landing and its release.

The workflow runs `release` and `release-pr` as separate jobs, which is the layout upstream supports; a single job running both is documented as "not recommended". The split matters for more than support, because only `release-pr` carries a `concurrency` group. GitHub cancels an existing *pending* run in a concurrency group as soon as another queues, whatever `cancel-in-progress` says, so a grouped `release` job could have its run discarded while waiting behind an earlier push. With `release_always = false` the merge of the release PR is the one commit that cuts a tag, and losing that run loses the tag with no failure anywhere. Serializing `release-pr` is still right: two runs writing the same release branch race, and a superseded one costs nothing because the next push redoes it.

`release-plz.toml` configures this:

- `git_only = true`: the crate is `publish = false`, so the previous version comes from git tags rather than crates.io.
- `publish = false`: nothing is published.
- `git_release_enable = true`, `git_release_type = "auto"`: every tag gets a GitHub release whose body is that version's changelog section, marked prerelease when the version has one. Changed on 2026-09-25; see `rejected.md`.
- `semver_check = false`: a binary exposes no public API, so cargo-semver-checks has nothing to compare.
- `release_always = false`: tag only when the release PR merges, not on every push to main.
- `release_commits = "^(feat|fix|perf|refactor)"`: release-plz decides from changed files, not from the changelog groups, so without this a `ci:` or `chore:` merge alone opens a release PR whose changelog section is empty (that is how v0.3.1 happened).
- `pr_body` replaces the default body, which carries a robot emoji and a generated-with footer.

## The first release

Release-plz anchors itself to git tags: with no `v*` tag, it treats the manifest version as an initial release and has nothing to diff against, so the first tag comes from outside the automation. `Cargo.toml` says 0.3.0 and the repo had no tag, so `v0.3.0` goes on the commit that carries that version:

```sh
git tag v0.3.0 <commit> && git push origin v0.3.0
```

after which everything is automatic.

## The release token

The workflow does not use `GITHUB_TOKEN` for writes: pushes, PRs and merges made with it trigger no workflow runs, so CI would never run on the release PR and its merge would never start the release run. Both jobs mint a one-hour token from the `lmarkmann-release` GitHub App instead, with only Contents and Pull requests write; the job's own `GITHUB_TOKEN` stays read-only.

The repository needs `RELEASE_APP_CLIENT_ID` (an Actions variable) and `RELEASE_APP_PRIVATE_KEY` (a secret), both set from 1Password (`GITHUB_RELEASE_APP` in the Developer vault) by the ci-vcm skill's `app_secrets.sh`, and the App installed on the repo. The App's key does not expire, so there is nothing to rotate on a calendar. Auto-merge needs the repository settings the same skill's `repo_setup.sh` writes: squash merges titled by the PR, auto-merge allowed, branches deleted on merge, and a ruleset requiring the `linux` check.

## Preview the next release

```sh
just release-preview
```

No release-plz subcommand has a dry-run flag, so the recipe runs the real `release-plz update`, prints the diff, and restores the tree afterwards. It refuses to start unless the tree is clean, untracked files included, because the restore ends in a `git clean` and that needs a known starting point.

## Making a clone publishable

To turn a clone into something publishable:

1. Drop `publish = false` from `Cargo.toml`, and add `license`.
2. Drop `publish = false` and `git_only = true` from `release-plz.toml`. The first is what actually stops the publish; the second tells release-plz to read the previous version from git tags instead of from the registry, which is only correct while nothing is published.
3. Give the `release` job `id-token: write` and register the repo as a crates.io trusted publisher; release-plz does the OIDC exchange itself, so there is no registry token. The first publish is manual.
4. Turn `semver_check` on at the same time if the crate grows a library; it was off because a binary has no public API.
