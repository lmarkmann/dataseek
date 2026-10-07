default:
    @just --list

fmt:
    cargo fmt --all
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged -- -D warnings

# Format check, lint, test.
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features --locked -- -D warnings
    cargo nextest run --all-features --locked
    cargo test --doc --locked

# Everything CI gates on, in the same order.
ci: check relevance bench-startup cross shear msrv audit
    typos

test *args:
    cargo nextest run {{ args }}

# Step through changed snapshots.
review:
    cargo insta review

# Accept every changed snapshot unread.
bless:
    cargo insta test --accept --unreferenced=reject

run *args:
    cargo run -- {{ args }}

build:
    cargo build --release

# Clippy for the Windows and macOS targets, as CI runs it.
cross:
    rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
    cargo clippy --all-targets --all-features --locked --target x86_64-pc-windows-msvc -- -D warnings
    cargo clippy --all-targets --all-features --locked --target aarch64-apple-darwin -- -D warnings

# Dependencies declared in Cargo.toml but never used.
shear:
    cargo shear

# Check the crate on the rust-version it claims.
msrv:
    cargo msrv verify -- cargo check --all-targets --all-features --locked

# Measure the real MSRV, below the declared one.
msrv-find:
    cargo msrv find -- cargo check --ignore-rust-version

# Line coverage, as a browsable report under target/llvm-cov/html.
cov *args:
    cargo llvm-cov nextest --html {{ args }}

# Criterion on this machine: `just bench --save-baseline main`, change, `just bench --baseline main`.
bench *args:
    cargo bench --features internals --bench search -- {{ args }}

# Startup latency of the release binary against its budgets; fails on a breach, warns on a regression against this machine's baseline. `--bless` rewrites that baseline.
bench-startup *args:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release --locked
    bin=target/release/dataseek
    mkdir -p docs/bench
    hyperfine -N --warmup 3 --runs 20 --export-json docs/bench/startup.json \
        "$bin --version" \
        "$bin --help" \
        "$bin" \
        "$bin completion fish" \
        "$bin sources"
    uv run --script scripts/bench_check.py docs/bench/startup.json {{ args }}

# Relevance of the merged ranking, scored offline against the judged snapshot; fails on a regression. `--bless`, `variants`, `pool`, `absorb`, `record`: docs/reference/development.md.
relevance *args:
    cargo run --quiet --locked --features internals --example relevance -- {{ args }}

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

[doc("Action and hook tags with newer versions available.")]
actions-outdated:
    #!/usr/bin/env bash
    set -euo pipefail

    if ! gh auth status >/dev/null 2>&1; then
      echo "Error: the GitHub CLI is not authenticated, so tags cannot be looked up." >&2
      echo "  Try:   gh auth login" >&2
      exit 1
    fi

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

[doc("Re-pin every action to its newest release at least a week old.")]
repin:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v pinact >/dev/null; then
      echo "Error: pinact is not installed, so the pins cannot be refreshed." >&2
      echo "  Try:   brew install pinact" >&2
      exit 1
    fi
    pinact run --update --min-age 7
    pinact run --verify --check

# Advisories, licenses, bans and sources.
deny:
    cargo deny check advisories licenses bans sources

# Static analysis of the workflows, offline.
zizmor:
    zizmor .github/workflows/

audit: deny zizmor

# Show the diff the next release PR would make, then restore the tree.
release-preview:
    #!/usr/bin/env bash
    set -euo pipefail

    if [[ -n $(git status --porcelain) ]]; then
      echo "Error: the working tree is dirty, so the preview could not be undone." >&2
      echo "  Try:   commit, stash, or remove untracked files first" >&2
      exit 1
    fi

    trap 'git reset --quiet && git checkout --quiet -- . && git clean --quiet -fd' EXIT

    release-plz update --config .github/release-plz.toml
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
