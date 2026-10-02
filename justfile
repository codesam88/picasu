# Picasu dev tasks
# Install just: cargo install just
# Activate pre-commit hook: git config core.hooksPath .githooks

[private]
help:
    @just --list --unsorted

# ── Backend ────────────────────────────────────────────────────────────────────

# cargo fmt
[group('backend')]
backend-format:
    cd backend && cargo fmt

# cargo fmt --check + cargo clippy
[group('backend')]
backend-check:
    cd backend && cargo fmt --check
    cargo clippy -- -D warnings -A clippy::unwrap_used

# cargo test
[group('backend')]
backend-test: frontend-build-maybe
    cd backend && cargo test

# cargo test (release)
[group('backend')]
backend-test-release: frontend-build-maybe
    cd backend && cargo test --release

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
    cd frontend && npx --no-install prettier --check . && npx --no-install vue-tsc --noEmit && npx --no-install vue-tsc --noEmit --project tsconfig.node.json && npx --no-install eslint .

# vitest run
[group('frontend')]
frontend-vitest:
    cd frontend && npm test

# run frontend playwright tests
[group('frontend')]
frontend-playwright: frontend-build-maybe
    #!/usr/bin/env bash
    set -e
    # Pre-build backend once so parallel test workers don't fight for the cargo build lock.
    # Skipped when PICASU_BINARY is already set (e.g. test-release supplies the release binary).
    [ -n "${PICASU_BINARY:-}" ] || cargo build --bin picasu
    # filter scenarios: npx playwright test --grep "onboarding"
    export PICASU_BINARY="${PICASU_BINARY:-$(pwd)/target/debug/picasu}"
    workers=$(( $(nproc) / 2 ))
    [ "$workers" -lt 1 ] && workers=1
    cd frontend && npx playwright test --workers="$workers"

# all frontend tests
[group('frontend')]
frontend-test: frontend-vitest frontend-playwright

# npm run build (npm ci + vue-tsc + vite build)
[group('frontend')]
frontend-build:
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

# cargo fmt --check + cargo clippy on utils/ crates
[group('utils')]
utils-check:
    cargo fmt --check -p snapfab -p paste -p openapi-sanity
    cargo clippy -p snapfab -p paste -p openapi-sanity -- -D warnings -A clippy::unwrap_used

# cargo test on utils/ crates
[group('utils')]
utils-test:
    cargo test -p snapfab -p openapi-sanity

# ── Tooling ─────────────────────────────────────────────────────────────────────

# Generate the checked-in public OpenAPI artifact from utoipa annotations
[group('utils')]
openapi-gen:
    RUST_MIN_STACK=16777216 cargo run --package picasu -- --dump-openapi > backend/openapi.json
    @echo "wrote backend/openapi.json"

# Three phases, in order: `openapi-sanity` checks the annotated source itself,
# `openapi-json-match` diffs the committed document against a fresh generation,
# and `openapi-routes-match` compares the routes the product build mounts with
# the same document. The source check comes first because the other two compare
# a document that has to be regenerated before either of them can say anything —
# a source defect found after the diff is a confusing way to be told about it.
# A dependency that fails stops the recipe, so any phase failing fails this one
# with that phase's diagnostics on stderr.
#
# Check that OpenAPI json matches routes registered in backend server
[group('utils')]
openapi-check: openapi-sanity openapi-json-match openapi-routes-match

# Source-level `#[utoipa::path]` checks: the annotation's shape (no restated
# path or verb, at least one response, one vocabulary tag, a doc comment with a
# one-line summary, no hand-set operation_id / summary / description), what it
# declares against what the route already binds (every declared parameter is one
# the route reads, its documented `required` matches the handler argument, its
# request_body names what `data = "…"` parses, and a form body declares
# multipart/form-data), and the handler body (every fallible guard's rejection
# has to be propagated). Reads the source, so it needs no build and no frontend
# bundle.
#
# The tool holds annotation conventions and no backend facts. A rule that needs a
# route path, a config value or a feature name belongs in the backend or in a
# recipe, not in `utils/openapi-sanity`. The one map it does hold is
# `GUARD_CLASSES` — guard class to binding name and to the status that class
# rejects with — which is a naming and status convention, not a fact read out of
# the backend.
#
# A8 is green on the tree. Its second branch is the one to know about: four
# `GuardTimestamp` bindings sit in a signature that already binds `?<timestamp>`,
# so they carry the class name as a word (`guard_timestamp`) rather than as the
# whole name. That count is pinned, so a route losing or gaining the collision fails
# a test rather than quietly changing what the branch applies to — see the A8 entry
# in section C of `.plan/openapi-annotation-checks.md`.
#
# `--expect-at-least` is the floor on annotated handlers the scan must see, with
# headroom below the tree's 63: a new handler must not break the gate, but a walk
# that stopped descending has to. The source root is absolute so that invoking
# this from a subdirectory checks the same tree; the tool prints it relative to
# the workspace root anyway.
[group('utils')]
openapi-sanity:
    cargo run --quiet -p openapi-sanity -- --source-root "{{justfile_directory()}}/backend/src/router" --expect-at-least 60

# Route-set parity: Match output of runtime Rocket route() output against
# last generated openapi.json output, ensuring that all routes are documented.
# Pinned to the feature set the release ships, so the build doing the
# checking is the build that ships — a feature-gated route only exists in the
# table of a build that has the feature. It embeds the frontend bundle, hence
# the build dependency.
[group('utils')]
[private]
openapi-routes-match: frontend-build-maybe
    cargo run --quiet --package picasu --features "embed-frontend auto-open-browser" -- \
        --check-openapi "{{justfile_directory()}}/backend/openapi.json"

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

# Auto-format .plan task frontmatter and body
[group('tooling')]
plan-format:
    cd frontend && npx --no-install prettier --write --no-error-on-unmatched-pattern '../.plan/**/*.md'

# Validate .plan task frontmatter structure
[group('tooling')]
plan-lint:
    plan --root {{justfile_directory()}} lint

# run plan <args>
[group('tooling')]
plan *args:
    plan --root {{justfile_directory()}} {{args}}

# ── Documentation ───────────────────────────────────────────────────────────────

# Generate OpenAPI spec and reference doc
[group('docs')]
docs-openapi: openapi-gen
    npx --yes widdershins@4.0.1 --summary backend/openapi.json -o docs/openapi-reference.md
    npx prettier --write docs/openapi-reference.md

# Build mdBook site at target/docs/
[group('docs')]
docs-build: docs-openapi
    #!/usr/bin/env bash
    set -e
    mkdir -p target/mdbook/src
    cp docs/book.toml target/mdbook
    cp -r docs/*.md target/mdbook/src
    mdbook build target/mdbook -d target/docs/book
    echo ""
    echo "=== Documentation built ==="
    echo "  Book:    target/docs/book/index.html"
    echo "  View:    just docs-serve → http://localhost:3637"

# Serve mdbook documentation
[group('docs')]
docs-serve:
    python3 -m http.server 3637 -d target/docs/book

# Format markdown files (README, docs, .plan, utils)
[group('docs')]
docs-format:
    cd frontend && npx --no-install prettier --write --no-error-on-unmatched-pattern '../*.md' '../docs/**/*.md' '../.plan/**/*.md' '../utils/**/*.md'

# Check markdown formatting (docs, .plan, utils)
[group('docs')]
docs-check:
    cd frontend && npx --no-install prettier --check --no-error-on-unmatched-pattern '../*.md' '../docs/**/*.md' '../.plan/**/*.md' '../utils/**/*.md'

# ── Global ─────────────────────────────────────────────────────────────────────

# Format everything (backend + utils + frontend + docs + .plan)
[group('global')]
format: backend-format utils-format frontend-format docs-format

# Run all linters and static checks
[group('global')]
check: backend-check utils-check frontend-check docs-check plan-lint openapi-check

# Run tests (backend + utils + frontend)
[group('global')]
test: backend-test utils-test frontend-test

# install dev tools + precommit hook
[group('global')]
setup-dev: install-dev
    git config core.hooksPath .githooks
    @echo "✓ Pre-commit hook enabled — ready to develop"

# Install plan tool from tablethat (pinned version)
[group('global')]
install-plan:
    cargo install --git https://github.com/tedsamhain/tablethat --rev df81155

# Install dev tools
[group('global')]
install-dev: install-plan
    cargo install sccache
    cargo install cargo-deny
    npm ci --prefix frontend

# Build frontend then backend (dev build)
[group('global')]
build: frontend-build backend-build

# Build frontend then backend with embedded assets (release)
[group('global')]
build-release: frontend-build backend-build-release

# Build + test release build - webroot is embedded
[group('global')]
test-release: backend-test-release build-release
    PICASU_BINARY=target/release/picasu just frontend-playwright

# Remove transient generated state (test runs, docs, sandbox)
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

# Build and run out of sandbox/{data,images}
[group('global')]
run: build
    #!/usr/bin/env sh
    set -e
    mkdir -p sandbox/images
    rm -rf sandbox/data
    cd backend && \
        PICASU_CONFIG_HOME="{{justfile_directory()}}/sandbox/data" \
        PICASU_DATA_HOME="{{justfile_directory()}}/sandbox/data" \
        PICASU_IMAGE_HOME="{{justfile_directory()}}/sandbox/images" \
        PICASU_WEB_ROOT="{{justfile_directory()}}/frontend/dist" \
        cargo run --bin picasu

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

    echo "[ precommit ] On '$branch' — format/lint enforced; run tests at your disgression."
    if echo "$changed" | grep -q '^backend/'; then
        just backend-check
        just openapi-check
    fi
    if echo "$changed" | grep -q '^utils/'; then
        just utils-check
        just utils-test
    fi
    if echo "$changed" | grep -qE '^(\.plan/|docs/|[^/]+\.md$|utils/.*\.md$)'; then
        just plan-lint
        just docs-check
    fi
    if echo "$changed" | grep -q '^frontend/'; then
        just frontend-check
    fi
