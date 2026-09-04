default:
    @just --list

fmt:
    cargo fmt --all
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged -- -D warnings

# The inner loop: format, lint, test. `ci` is the full gate.
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features --locked -- -D warnings
    cargo nextest run --all-features --locked

# Everything CI gates on, in the same order. See docs/development.md.
ci: check shear msrv audit

test *args:
    cargo nextest run {{ args }}

# Inspect changed snapshots one by one; `bless` takes them all unread.
review:
    cargo insta review

bless:
    cargo insta test --accept --unreferenced=reject

run *args:
    cargo run -- {{ args }}

build:
    cargo build --release

# Dependencies declared in Cargo.toml but never used.
shear:
    cargo shear

# Does the crate still compile on the rust-version it claims? The trailing check
# command is what makes this match the CI job; cargo-msrv's default is a bare
# `cargo check`, which skips tests, examples and features.
msrv:
    cargo msrv verify -- cargo check --all-targets --all-features --locked

# What the MSRV actually is; the trailing check command is what makes it look below the claim.
msrv-find:
    cargo msrv find -- cargo check --ignore-rust-version

# Line coverage, as a browsable report under target/llvm-cov/html.
cov *args:
    cargo llvm-cov nextest --html {{ args }}

# Mutate what the branch changed; a survivor is a line the tests run but never check.
mutants *args:
    #!/usr/bin/env bash
    set -euo pipefail
    diff=target/branch.diff
    mkdir -p target
    git diff --merge-base main > "$diff"
    if [[ ! -s $diff ]]; then
      echo "Nothing changed against main, so there is nothing to mutate." >&2
      exit 0
    fi
    cargo mutants --in-diff "$diff" --test-tool nextest {{ args }}

# Where the release binary's size goes; the profile already strips and thin-LTOs.
bloat *args:
    cargo bloat --release --crates {{ args }}

# Crates with newer versions available.
crates-outdated:
    cargo outdated --root-deps-only

# Compares only the components a pin declares, so @v2 is current until a v3 exists,
# @v0.5 until a v0.6, and @v1.48.0 the moment v1.48.1 ships. Branch pins (@stable,
# @master) move on their own and are skipped. A hash pin is compared through the
# version comment pinact writes beside it, which is why that comment is load-bearing
# rather than decoration. See docs/development.md.
[doc("Action and hook tags with newer versions available.")]
actions-outdated:
    #!/usr/bin/env bash
    set -euo pipefail

    if ! gh auth status >/dev/null 2>&1; then
      echo "Error: the GitHub CLI is not authenticated, so tags cannot be looked up." >&2
      echo "  Try:   gh auth login" >&2
      exit 1
    fi

    # Not deduplicated by repository on purpose. typos is pinned in both ci.yml and
    # prek.toml and the two are meant to agree, so a disagreement should surface as
    # two rows rather than collapse into one.
    pins=$({
      grep -rhoE 'uses: [A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+([[:space:]]*#[[:space:]]*[A-Za-z0-9_.-]+)?' .github/workflows/ |
        sed 's/^uses: //'
      awk -F'"' '
        /^repo = "https:\/\/github\.com\// { slug = $2; sub(".*github\\.com/", "", slug) }
        /^rev = "/ && slug                 { print slug "@" $2; slug = "" }
      ' prek.toml
    } | sort -u)

    behind=0
    while read -r pin comment; do
      slug=${pin%@*}
      ref=${pin##*@}

      # A hash pin carries its version in the trailing comment, so read the comment
      # as the ref. Without this the 40-character SHA fails the version test below
      # and the action drops out of the report unnoticed, which would make pinning
      # the one change that leaves this repo less current than before.
      if [[ $ref =~ ^[0-9a-f]{40}$ ]]; then
        ref=${comment#\#}
        ref=${ref# }
        if [[ -z $ref ]]; then
          echo "Error: $slug is hash-pinned with no version comment to compare." >&2
          echo "  Try:   pinact run --update .github/workflows/" >&2
          behind=1
          continue
        fi
      fi

      [[ $ref =~ ^v?[0-9]+(\.[0-9]+)*$ ]] || continue

      latest=$(gh api "repos/$slug/releases/latest" --jq .tag_name 2>/dev/null) ||
        latest=$(gh api "repos/$slug/tags" --jq '.[0].name' 2>/dev/null) ||
        { echo "Error: $slug publishes neither releases nor tags." >&2; continue; }

      pinned=${ref#v}
      depth=$(awk -F. '{ print NF }' <<<"$pinned")
      if [[ $pinned != "$(cut -d. -f1-"$depth" <<<"${latest#v}")" ]]; then
        printf '%-30s %-10s -> %s\n' "$slug" "$ref" "$latest"
        behind=1
      fi
    done <<<"$pins"

    (( behind == 1 )) || echo "Action and hook pins are current."

# Everything this repo pins, crates and tags alike.
outdated: crates-outdated actions-outdated

# Re-pin the one hash-pinned workflow. A recipe rather than a line in the docs
# because the exclusion is not optional: dtolnay/rust-toolchain@stable names a
# toolchain rather than a version, and a run without it errors on that line.
# --verify afterwards is what catches a SHA whose version comment has drifted,
# which is the failure `just actions-outdated` cannot see for itself.
[doc("Re-pin release-plz.yml to the latest action releases.")]
repin:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v pinact >/dev/null; then
      echo "Error: pinact is not installed, so the pins cannot be refreshed." >&2
      echo "  Try:   brew install pinact" >&2
      exit 1
    fi
    pinact run --update --exclude 'dtolnay/rust-toolchain' .github/workflows/release-plz.yml
    pinact run --verify --check --exclude 'dtolnay/rust-toolchain' .github/workflows/release-plz.yml

# See docs/security.md.
deny:
    cargo deny check advisories licenses bans sources

# See docs/security.md.
zizmor:
    zizmor .github/workflows/

# See docs/security.md.
audit: deny zizmor

# No release-plz subcommand has a dry-run flag, so a preview has to run the real
# update and put the tree back. It refuses to start unless the tree is clean,
# untracked files included, because the restore is a checkout plus a clean and
# both need a known starting point. See docs/release.md.
release-preview:
    #!/usr/bin/env bash
    set -euo pipefail

    if [[ -n $(git status --porcelain) ]]; then
      echo "Error: the working tree is dirty, so the preview could not be undone." >&2
      echo "  Try:   commit, stash, or remove untracked files first" >&2
      exit 1
    fi

    # Restores on every exit path, including release-plz failing part-way through.
    trap 'git reset --quiet && git checkout --quiet -- . && git clean --quiet -fd' EXIT

    release-plz update
    # A generated CHANGELOG.md lands untracked, and git diff does not show
    # untracked files; --intent-to-add puts it in the diff without staging it.
    git add --intent-to-add --quiet .
    git --no-pager diff

# Keep package metadata, CLI help, README, and GitHub in step.
description TEXT:
    #!/usr/bin/env bash
    set -euo pipefail
    description={{ quote(TEXT) }}

    if [[ -z ${description//[[:space:]]/} ]]; then
      echo "Error: the description cannot be empty." >&2
      echo "  Try:   just description \"WHAT THE CLI DOES\"" >&2
      exit 1
    fi
    if [[ $description == *$'\n'* || $description == *$'\r'* ]]; then
      echo "Error: the description must fit on one line." >&2
      exit 1
    fi
    if ! grep -q '^description = "' Cargo.toml; then
      echo "Error: Cargo.toml has no package description to update." >&2
      exit 1
    fi
    if ! grep -q '^<!-- repo-description -->$' README.md ||
       ! grep -q '^<!-- /repo-description -->$' README.md; then
      echo "Error: README.md has no repository description markers." >&2
      exit 1
    fi

    DESCRIPTION="$description" perl -0pi -e '
      BEGIN {
        $description = $ENV{DESCRIPTION};
        $description =~ s/\\/\\\\/g;
        $description =~ s/"/\\"/g;
      }
      s/^description = ".*"$/description = "$description"/m;
    ' Cargo.toml
    DESCRIPTION="$description" perl -0pi -e '
      s{(?<=<!-- repo-description -->\n).*?(?=\n<!-- /repo-description -->)}
       {$ENV{DESCRIPTION}}s;
    ' README.md
    gh repo edit --description "$description"

    echo "Updated Cargo.toml, README.md, CLI help, and the GitHub repository description."

# >>> rename
# Single-use recipe: see docs/setup.md.
rename NEW:
    #!/usr/bin/env bash
    set -euo pipefail
    new={{ quote(NEW) }}
    old=$(sed -n 's/^name = "\(.*\)"/\1/p' Cargo.toml | head -1)

    if ! [[ $new =~ ^[a-z][a-z0-9]*([-_][a-z0-9]+)*$ ]]; then
      echo "Error: '$new' is not a usable crate name." >&2
      echo "  Try:   lowercase, starting with a letter, words joined by - or _" >&2
      exit 1
    fi
    if [[ $new == "$old" ]]; then
      echo "Error: the crate is already named '$new'." >&2
      exit 1
    fi
    if [[ -n $(git status --porcelain) ]]; then
      echo "Error: the working tree is dirty." >&2
      echo "  Try:   commit or stash first, so the rename is reviewable on its own" >&2
      exit 1
    fi

    # Ask git which files carry the name rather than listing them by hand. A literal
    # list goes stale the moment a file is added, and it did: tests/*.rs never matched
    # tests/cli/filters.rs, so every clone kept the template's name in its test tree
    # while the test still passed. Scoped to the paths that describe the clone's own
    # binary; the exclusions below are the places the old name is still correct.
    keep='^(README\.md|docs/setup\.md|docs/rejected\.md)$'
    files=$(git grep -l --fixed-strings -- "$old" Cargo.toml src tests docs | grep -Ev "$keep")
    if [[ -z $files ]]; then
      echo "Error: '$old' does not appear anywhere; is Cargo.toml's name field right?" >&2
      exit 1
    fi
    echo "$files" | tr '\n' ' ' | xargs perl -pi -e "s/\Q$old\E/$new/g"

    cargo insta test --accept --unreferenced=reject

    # The list above is derived, so it cannot drift, but the exclusions are still a
    # judgment call. Fail loudly if the old name survives anywhere else, which is the
    # check that would have caught tests/cli/filters.rs on its first run.
    if leftover=$(git grep -n --fixed-strings -- "$old" -- . ':!README.md' ':!docs/setup.md' ':!docs/rejected.md'); then
      echo "Error: '$old' still appears in files the rename did not cover:" >&2
      echo "$leftover" >&2
      echo "  Try:   git checkout -- . to undo, then add the path to the rename recipe" >&2
      exit 1
    fi

    perl -ni -e 'print unless /^# >>> rename$/ .. /^# <<< rename$/' justfile

    echo
    echo "Renamed $old -> $new."
    echo "Next: just description \"WHAT THE CLI DOES\""
    echo "Then replace the count subcommand with your own."
# <<< rename
