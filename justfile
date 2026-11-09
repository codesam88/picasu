# Picasu dev tasks
# Install just: cargo install just
# Activate pre-commit hook: git config core.hooksPath .githooks
# Recipe comments are single-line summaries; keep `just --list` within 80 columns.

[private]
help:
    @just --list --unsorted

# ── Backend ────────────────────────────────────────────────────────────────────

# cargo fmt
[group('backend')]
backend-format:
    cd backend && cargo fmt

# cargo fmt --check + clippy (all targets)
[group('backend')]
backend-check:
    cd backend && cargo fmt --check
    cargo clippy --all-targets -- -D warnings

[private]
nextest-present:
    #!/usr/bin/env sh
    command -v cargo-nextest >/dev/null || { echo "cargo-nextest is missing — run: just install nextest" >&2; exit 1; }

# Backend tests (cargo-nextest)
[group('backend')]
backend-test: frontend-build-maybe nextest-present
    cd backend && PICASU_WEAK_PWHASH=1 cargo nextest run && cargo test --doc

# Backend tests in release mode
[group('backend')]
backend-test-release: frontend-build-maybe nextest-present
    cd backend && cargo nextest run --release && cargo test --doc --release

# cargo deny
[group('backend')]
backend-audit:
    cargo deny check

# cargo build (dev build)
[group('backend')]
backend-build:
    cd backend && cargo build

# cargo build (release build)
[group('backend')]
backend-build-release:
    cd backend && cargo build --release --features embed-frontend

# ── Frontend ───────────────────────────────────────────────────────────────────

# prettier --write
[group('frontend')]
frontend-format:
    cd frontend && npx --no-install prettier --write .

# prettier --check + vue-tsc + eslint
[group('frontend')]
frontend-check:
    cd frontend && npx --no-install prettier --check . && (npx --no-install vue-tsc --noEmit & p1=$!; npx --no-install vue-tsc --noEmit --project tsconfig.node.json & p2=$!; wait $p1 && wait $p2) && npx --no-install eslint .

# vitest run
[group('frontend')]
frontend-vitest:
    cd frontend && npm test

# run playwright tests (optional filter)
[group('frontend')]
frontend-playwright filter="": frontend-build-maybe
    #!/usr/bin/env bash
    set -e
    # Pre-build backend once so parallel test workers don't fight for the cargo build lock.
    # Skipped when PICASU_BINARY is already set (e.g. test-release supplies the release binary).
    [ -n "${PICASU_BINARY:-}" ] || cargo build --bin picasu
    export PICASU_BINARY="${PICASU_BINARY:-$(pwd)/target/debug/picasu}"
    # Pre-build snapfab once so e2e scenarios spawn the binary directly instead
    # of `cargo run` per scenario (avoids per-spawn cargo check overhead).
    # Skipped when SNAPFAB_BINARY is already set.
    [ -n "${SNAPFAB_BINARY:-}" ] || cargo build -p snapfab
    export SNAPFAB_BINARY="${SNAPFAB_BINARY:-$(pwd)/target/debug/snapfab}"
    # Each worker runs a browser and a backend, so half the cores is the
    # ceiling that keeps the runner from thrashing; the count follows the
    # machine (a 4-core CI runner gets 2 workers, an 8-core dev box 4).
    # PLAYWRIGHT_WORKERS overrides it.
    if [ -n "${PLAYWRIGHT_WORKERS:-}" ]; then
        workers="$PLAYWRIGHT_WORKERS"
    else
        workers=$(( $(nproc) / 2 ))
        [ "$workers" -lt 1 ] && workers=1
    fi
    if [ -n "{{filter}}" ]; then
        cd frontend && npx playwright test --workers="$workers" --grep "{{filter}}"
    else
        cd frontend && npx playwright test --workers="$workers"
    fi

# all frontend tests
[group('frontend')]
frontend-test: frontend-vitest frontend-playwright

[private]
frontend-ci-maybe:
    cd frontend && (test node_modules/.package-lock.json -nt package.json -a node_modules/.package-lock.json -nt package-lock.json || npm ci)

# npm run build; resyncs deps when package.json/lock changed since install
[group('frontend')]
frontend-build: frontend-ci-maybe
    cd frontend && npm run build

[private]
frontend-build-maybe:
    test -d frontend/dist/assets || just frontend-build

# npm audit
[group('frontend')]
frontend-audit:
    cd frontend && npm audit --omit=dev

# ── Utils (snapfab, paste, openapi-sanity) ────────────────────────────────────

# cargo fmt on utils/ crates
[group('utils')]
utils-format:
    cargo fmt -p snapfab -p paste -p openapi-sanity

# utils fmt + clippy (all targets)
[group('utils')]
utils-check: zizmor-check
    cargo fmt --check -p snapfab -p paste -p openapi-sanity
    cargo clippy --all-targets -p snapfab -p paste -p openapi-sanity -- -D warnings

# cargo test utils/ + Schemathesis derivations
[group('utils')]
utils-test:
    cargo test -p snapfab -p openapi-sanity
    python3 {{justfile_directory()}}/utils/schemathesis/test_auth_rules.py

# Benchmark tooling submodule (utils/bench/justfile); `just bench <recipe>`
# works from any directory. Old flat names kept as aliases.
[group('utils')]
mod bench "utils/bench/justfile"

# ── Tooling ─────────────────────────────────────────────────────────────────────

# Regenerate the checked-in OpenAPI artifact
[group('utils')]
openapi-gen:
    RUST_MIN_STACK=16777216 cargo run --package picasu -- --dump-openapi > backend/openapi.json
    @echo "wrote backend/openapi.json"

# Run all OpenAPI checks.
[group('utils')]
openapi-check: openapi-sanity openapi-json-match openapi-breakcheck openapi-lint openapi-routes-match auth-matrix-check

# run dylint auth lints (advisory)
[group('utils')]
dylint-check:
    cargo dylint --all -- -p picasu

# Lint the OpenAPI document (errors fail)
[group('utils')]
openapi-lint:
    cd frontend && npx --no-install spectral lint --ruleset ../.spectral.yaml ../backend/openapi.json

# check OpenAPI annotation conventions
[group('utils')]
openapi-sanity:
    cargo run --quiet -p openapi-sanity -- --source-root "{{justfile_directory()}}/backend/src/router" --app-error-map "{{justfile_directory()}}/backend/src/error.rs" --expect-at-least 60

# Compare mounted Rocket routes with documented OpenAPI operations.
[group('utils')]
[private]
openapi-routes-match: frontend-build-maybe
    cargo run --quiet --package picasu --features "embed-frontend auto-open-browser" -- \
        --check-openapi "{{justfile_directory()}}/backend/openapi.json"

# Run the already-built release binary's route-set gate against the committed spec.
[group('utils')]
openapi-routes-release:
    {{justfile_directory()}}/target/release/picasu --check-openapi "{{justfile_directory()}}/backend/openapi.json"

# Ensure that committed/staged json matches current --dump-openapi output
[private]
openapi-json-match:
    #!/usr/bin/env bash
    set -euo pipefail
    generated="$(mktemp)"
    trap 'rm -f "$generated"' EXIT
    RUST_MIN_STACK=16777216 cargo run -q --package picasu -- --dump-openapi > "$generated"
    if ! diff -u backend/openapi.json "$generated"; then
        echo ""
        echo "backend/openapi.json is out of date with the utoipa annotations."
        echo "Run 'just openapi-gen' and commit the result."
        exit 1
    fi
    echo "openapi.json matches the generated spec"

# Install one managed tool: `just install <name>` (e.g. `just install ffmpeg`).
# dispatcher loops scripts/deps/*.sh
[group('deps')]
install tool:
    ./scripts/deps-install.sh --tool {{tool}}

# Common fuzz flows; the fundamental commands (raw launcher, campaign engine,
# tier-1 check) are scripts under scripts/ and utils/schemathesis/, combined
# as documented in utils/schemathesis/README.md.

# tier-1 gate: seeded sweep, baseline-compared
[group('utils')]
fuzz-check: frontend-build-maybe (install "schemathesis")
    #!/usr/bin/env bash
    set -euo pipefail
    ./scripts/deps-install.sh --tool schemathesis --check
    utils/schemathesis/check.sh

# Serve a seeded instance in the foreground (first run seeds; Ctrl-C stops)
# run fuzz matrix (auth checks)
[group('utils')]
fuzz-matrix: frontend-build-maybe
    utils/schemathesis/fuzz_matrix.sh

[group('utils')]
fuzz-serve *args: frontend-build-maybe
    utils/schemathesis/serve.sh {{ args }}

# run fuzz campaign (budget via FUZZ_MIN)
[group('utils')]
fuzz-run *args: (install "schemathesis")
    #!/usr/bin/env bash
    set -euo pipefail
    ./scripts/deps-install.sh --tool schemathesis --check
    FUZZ_STATE_DIR="${FUZZ_STATE_DIR:-{{justfile_directory()}}/sandbox/fuzz}" \
    utils/schemathesis/campaign.sh {{ args }}


# audit workflows/deps offline (no GH_TOKEN)
[group('utils')]
zizmor-check: (install "zizmor")
    #!/usr/bin/env bash
    set -euo pipefail
    ./scripts/deps-install.sh --tool zizmor --check
    ./scripts/zizmor-run.sh --offline --no-progress .

# Typecheck + test the eventual-consistency spec (Node-only, no JDK)
[group('utils')]
quint-check: (install "quint")
    quint typecheck utils/quint/pipeline/eventual-consistency.qnt
    quint test utils/quint/pipeline/eventual-consistency.qnt

# Non-blocking by design: MSRV floor check (nightly lane).
[group('utils')]
msrv-check:
    #!/usr/bin/env bash
    set -euo pipefail
    floor="$(grep '^rust-version' "{{justfile_directory()}}/backend/Cargo.toml" | cut -d'"' -f2)"
    rustup toolchain install "$floor" --profile minimal
    cd "{{justfile_directory()}}"
    cargo +"$floor" check --workspace --locked --all-targets

# Online audit for the scheduled CI job: stale pins, archived actions and
# other checks that need the GitHub API. Needs GH_TOKEN (or GITHUB_TOKEN);
# without one it degrades to the offline check. Takes extra zizmor flags,
# e.g. --format=sarif
[group('utils')]
zizmor-check-online *args="": (install "zizmor")
    #!/usr/bin/env bash
    set -euo pipefail
    ./scripts/deps-install.sh --tool zizmor --check
    ./scripts/zizmor-run.sh {{ args }} --no-progress .

# Fail on breaking OpenAPI changes
[group('utils')]
openapi-breakcheck: (install "oasdiff")
    #!/usr/bin/env bash
    set -euo pipefail
    ./scripts/deps-install.sh --tool oasdiff --check
    ./scripts/openapi-breakcheck.sh "{{ justfile_directory() }}"

# check PICASU_* env vars match docs/config.md
[group('utils')]
env-var-check:
    node --test {{justfile_directory()}}/scripts/env-var-lint.test.mjs
    node {{justfile_directory()}}/scripts/env-var-lint.mjs {{justfile_directory()}}

# ensure auth-matrix artifacts match generator
[group('utils')]
auth-matrix-check:
    #!/usr/bin/env bash
    set -euo pipefail
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT
    mkdir -p "$tmpdir/backend"
    cp "{{justfile_directory()}}/backend/openapi.json" "$tmpdir/backend/openapi.json"
    node "{{justfile_directory()}}/scripts/auth-matrix.mjs" "$tmpdir"
    if ! diff -u "{{justfile_directory()}}/docs/auth-matrix.md" "$tmpdir/docs/auth-matrix.md"; then
        echo ""
        echo "docs/auth-matrix.md is out of date with the generator."
        echo "Run 'node scripts/auth-matrix.mjs .' and commit the result."
        exit 1
    fi
    if ! diff -u "{{justfile_directory()}}/backend/auth-matrix.json" "$tmpdir/backend/auth-matrix.json"; then
        echo ""
        echo "backend/auth-matrix.json is out of date with the generator."
        echo "Run 'node scripts/auth-matrix.mjs .' and commit the result."
        exit 1
    fi
    echo "auth-matrix artifacts match the generated output"

# Install 'plan' tool (pinned)
[group('deps')]
install-plan:
    ./scripts/deps-install.sh --tool plan

# Auto-format .plan task frontmatter and body
[group('utils')]
plan-format:
    plan format

# check .plan task frontmatter correctness
[group('utils')]
plan-check:
    plan lint
    plan format --check

# Validate the eventual-consistency event catalog
[group('utils')]
events-check:
    node {{justfile_directory()}}/utils/quint/events-check.mjs {{justfile_directory()}}/utils/quint/pipeline

# Validate the goals, requirements, assumptions and scope catalogs + cross-catalog references
[group('utils')]
catalog-check:
    node {{justfile_directory()}}/utils/quint/catalog-check.mjs {{justfile_directory()}}/utils/quint/pipeline

# Regenerate docs/eventual-consistency.md from the JSON catalogs
[group('utils')]
docs-eventual-consistency:
    node {{justfile_directory()}}/utils/quint/gen-eventual-consistency-doc.mjs {{justfile_directory()}}/utils/quint/pipeline {{justfile_directory()}}
    cd {{justfile_directory()}}/frontend && npx --no-install prettier --write --no-error-on-unmatched-pattern '../docs/eventual-consistency.md'

# Closed-world audit over utils/quint/pipeline/*.qnt: every definition
# carries a resolving tag, every registry ID is cited. Regenerate the
# traceability map (utils/quint/pipeline/traceability.json) with:
# node utils/quint/specs-check.mjs utils/quint/pipeline --generate
[group('utils')]
spec-check:
    node {{justfile_directory()}}/utils/quint/specs-check.mjs {{justfile_directory()}}/utils/quint/pipeline

# ensure all test/check rules are called from CI and global rules
[group('utils')]
gates-check:
    node --test {{justfile_directory()}}/scripts/quality-gates.test.mjs
    node {{justfile_directory()}}/scripts/quality-gates.mjs {{justfile_directory()}}

# run plan <args> (needs install-plan)
[group('utils')]
plan *args:
    plan --root {{justfile_directory()}} {{args}}

# ── Documentation ───────────────────────────────────────────────────────────────

# Generate OpenAPI spec and reference doc
[group('docs')]
docs-openapi: openapi-gen
    npx --yes widdershins@4.0.1 --summary backend/openapi.json -o docs/openapi-reference.md
    plan format docs/openapi-reference.md

# Regenerate the gates doc (checked by gates-check)
[group('docs')]
gates-docs:
    node {{justfile_directory()}}/scripts/quality-gates.mjs {{justfile_directory()}} --generate docs/quality-gates.md

# Build mdBook site at target/docs/
[group('docs')]
docs-build: docs-openapi (install "mdbook")
    #!/usr/bin/env bash
    set -e
    mkdir -p target/mdbook/src
    cp docs/book.toml target/mdbook
    cp -r docs/*.md target/mdbook/src
    cp -r docs/briefs target/mdbook/src/briefs
    mdbook build target/mdbook -d target/docs/book
    echo ""
    echo "=== Documentation built ==="
    echo "  Book:    target/docs/book/index.html"
    echo "  View:    just docs-serve → http://localhost:3637"

# Serve mdbook documentation
[group('docs')]
docs-serve:
    python3 -m http.server 3637 -d target/docs/book

# format markdown (README, docs, .plan, utils)
[group('docs')]
docs-format:
    plan format docs
    find . -maxdepth 1 -name \*.md |xargs plan format
    find utils scripts -name \*.md |xargs plan format

# check markdown (docs, .plan, utils)
[group('docs')]
docs-check:
    plan format --check docs
    find . -maxdepth 1 -name \*.md |xargs plan format --check
    find utils scripts -name \*.md |xargs plan format --check

# ── Global ─────────────────────────────────────────────────────────────────────

# Format everything
[group('global')]
format: backend-format utils-format frontend-format docs-format plan-format

# Run all linters and static checks
[group('global')]
check: backend-check utils-check frontend-check docs-check plan-check events-check catalog-check spec-check openapi-check env-var-check zizmor-check gates-check quint-check branches-prune-test

# run full test suite
[group('global')]
test: backend-test utils-test frontend-test fuzz-matrix

# verify installed tool versions
[group('deps')]
check-tools:
    ./scripts/deps-install.sh --check

# install managed dev tools
[group('deps')]
install-dev:
    ./scripts/deps-install.sh
    npm ci --prefix frontend

# Generate gitignored .cargo/sccache.toml when sccache is installed
[group('deps')]
setup-sccache:
    #!/usr/bin/env bash
    set -euo pipefail
    f="{{justfile_directory()}}/.cargo/sccache.toml"
    if ! command -v sccache >/dev/null 2>&1; then
        rm -f "$f"
        exit 0
    fi
    mkdir -p "$HOME/.cache/sccache"
    printf '%s\n' \
      '[build]' \
      'rustc-wrapper = "sccache"' \
      'incremental = false' \
      '' \
      '[env]' \
      "SCCACHE_DIR = \"$HOME/.cache/sccache\"" \
      'SCCACHE_CACHE_SIZE = "10G"' \
      'SCCACHE_IGNORE_SERVER_IO_ERROR = "1"' > "$f"
    echo "✓ sccache enabled via $f"

# install dev tools + precommit hook
[group('deps')]
setup-dev: install-dev setup-sccache
    git config core.hooksPath .githooks
    @echo "✓ Pre-commit hook enabled — ready to develop"

# Build frontend then backend (dev build)
[group('global')]
build: frontend-build backend-build

# Release build with embedded assets
[group('global')]
build-release: frontend-build backend-build-release

# build + test release (webroot embedded)
[group('global')]
test-release: backend-test-release build-release
    PICASU_BINARY=target/release/picasu just frontend-playwright

# Remove transient generated state
[group('global')]
clean:
    rm -rf .testruns/*
    rm -rf target/mdbook
    rm -rf sandbox/data

# Clean all generated files
[group('global')]
distclean: clean
    rm -rf target/docs
    rm -rf frontend/dist

# Prune stale local/remote branches (report; --apply + --expire-* to delete)
[group('global')]
branches-prune *args:
    node scripts/prune-branches.mjs {{args}}

# Test the branch pruning helper against a fixture repository
[group('global')]
branches-prune-test:
    node --test {{justfile_directory()}}/scripts/prune-branches.test.mjs

# Build and run out of sandbox/{data,images}
[group('global')]
run: build
    #!/usr/bin/env sh
    set -e
    mkdir -p sandbox/images
    mkdir -p sandbox/users
    rm -rf sandbox/data
    # Dev-sandbox-only KDF effort (matches the e2e launcher); production default untouched.
    cd backend && \
        PICASU_BOOTSTRAP_ADMIN=true \
        PICASU_WEAK_PWHASH=1 \
        PICASU_CONFIG_HOME="{{justfile_directory()}}/sandbox/data" \
        PICASU_DATA_HOME="{{justfile_directory()}}/sandbox/data" \
        PICASU_USERS_HOME="{{justfile_directory()}}/sandbox/users" \
        PICASU_PERSONAL_ALBUMS=true \
        PICASU_IMAGE_HOME="{{justfile_directory()}}/sandbox/images" \
        PICASU_WEB_ROOT="{{justfile_directory()}}/frontend/dist" \
        cargo run --bin picasu -- --bootstrap-admin

# Run security audits (backend + frontend)
[group('global')]
audit: backend-audit frontend-audit

# Run format, linter, static checks and tests
[group('global')]
precommit:
    #!/usr/bin/env sh
    set -e
    branch=$(git rev-parse --abbrev-ref HEAD)
    changed=$(git diff --cached --name-only)

    if [ "$branch" = "main" ]; then
        echo "[ precommit ] On main — full test suite is required to pass."
        just check
        just test
        exit 0
    fi

    echo "[ precommit ] On '$branch' — format/lint enforced; targeted checks/tests run for changed areas."
    if echo "$changed" | grep -q '^backend/'; then
        just backend-check
        just openapi-check
    fi
    if echo "$changed" | grep -qE '^\.(spectral\.yaml|oasdiff-ignore\.txt)$'; then
        # Root files that configure the openapi gate run the gate they configure.
        just openapi-check
    fi
    if echo "$changed" | grep -q '^utils/'; then
        just utils-check
    fi
    if echo "$changed" | grep -qE '^(backend|utils)/'; then
        just utils-test
    fi
    if echo "$changed" | grep -qE '^(\.plan/|docs/|[^/]+\.md$|utils/.*\.md$)'; then
        just plan-check
        just docs-check
    fi
    if echo "$changed" | grep -q '^frontend/'; then
        just frontend-check
    fi
    if echo "$changed" | grep -qE '^(backend/|docs/config\.md$|README\.md$)'; then
        just env-var-check
    fi
