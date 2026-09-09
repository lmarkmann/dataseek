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

