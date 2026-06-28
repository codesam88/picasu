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
    cd backend && cargo fmt --check && cargo clippy -- -D warnings -A clippy::unwrap_used

# cargo nextest run
[group('backend')]
backend-test:
    cd backend && cargo nextest run

# cargo nextest run (release)
[group('backend')]
backend-test-release:
    cd backend && cargo nextest run --release

# cargo deny check
[group('backend')]
backend-deny:
    cd backend && cargo deny check

# cargo audit
[group('backend')]
backend-audit:
    cd backend && cargo audit

# cargo build (debug, no embedded frontend) — developer default; matches check/test
[group('backend')]
backend-build:
    cd backend && cargo build

# cargo build --release --features embed-frontend — production build (CI/deployment)
[group('backend')]
backend-build-release:
    cd backend && cargo build --release --features embed-frontend

# ── Frontend ───────────────────────────────────────────────────────────────────

# prettier --write
[group('frontend')]
frontend-format:
    npx prettier --write frontend/

# prettier --check + vue-tsc + eslint
[group('frontend')]
frontend-check:
    npx prettier --check frontend/ && cd frontend && npx vue-tsc --noEmit && npx eslint .

# vitest run
[group('frontend')]
frontend-vitest:
    cd frontend && npm test

# Playwright E2E scenarios (each scenario starts its own isolated backend)
[group('frontend')]
frontend-playwright:
    # filter scenarios: npx playwright test --grep "onboarding"
    cd frontend && npm run test:e2e

# all frontend tests
[group('frontend')]
frontend-test: frontend-vitest frontend-playwright

# npm run build (npm ci + vue-tsc + vite build)
[group('frontend')]
frontend-build:
    cd frontend && npm run build

# npm audit
[group('frontend')]
frontend-audit:
    cd frontend && npm audit

# ── Xtask tooling ───────────────────────────────────────────────────────────────

# Generate openapi.json + markdown reference from utoipa annotations
[group('xtask')]
openapi-gen:
    RUST_MIN_STACK=16777216 cargo run --package picasu -- --dump-openapi > backend/openapi.json
    @echo "wrote backend/openapi.json"

# Generate full API docs: openapi.json + markdown reference
[group('xtask')]
openapi-docs: openapi-gen
    npx --yes widdershins --summary backend/openapi.json -o docs/openapi-reference.md
    npx prettier --write docs/openapi-reference.md

# Verify generated docs match annotations (CI / precommit)
[group('xtask')]
openapi-docs-check: openapi-docs
    git diff --exit-code docs/openapi-reference.md

# Auto-format .plan task frontmatter and body
[group('xtask')]
plan-format:
    cargo xtask plan --format

# Validate .plan task frontmatter structure
[group('xtask')]
plan-lint:
    cargo xtask plan --lint

# List / filter / search .plan tasks (passes through all flags — e.g. `just plan -k`, `just plan -s open`)
[group('xtask')]
plan *args:
    cargo xtask plan {{args}}

# ── Documentation ───────────────────────────────────────────────────────────────

# Format markdown files (README, docs, .plan)
[group('docs')]
docs-format:
    npx prettier --write --no-error-on-unmatched-pattern '*.md' 'docs/**/*.md' '.plan/**/*.md'

# Check markdown formatting (precommit / CI)
[group('docs')]
docs-check:
    npx prettier --check --no-error-on-unmatched-pattern '*.md' 'docs/**/*.md' '.plan/**/*.md'

# ── Global ─────────────────────────────────────────────────────────────────────

# Format everything (backend + frontend + docs + .plan)
[group('global')]
format: backend-format frontend-format docs-format plan-format

# Run linters, including format checks (backend + frontend + docs + .plan)
[group('global')]
check: backend-check frontend-check docs-check plan-lint

# Run tests (backend + frontend)
[group('global')]
test: backend-test frontend-test

# One-shot: install tooling + enable pre-commit hook for a fresh clone
[group('global')]
setup-dev: install-dev
    git config core.hooksPath .githooks
    @echo "✓ Pre-commit hook enabled — ready to develop"

# Install all dev tooling (cargo tools + frontend deps including prettier)
[group('global')]
install-dev:
    cargo install sccache
    cargo install cargo-deny cargo-audit
    cargo install --locked cargo-nextest
    npm ci --prefix frontend
    npm install --prefix frontend --save-dev --save-exact widdershins

# Build frontend then backend (debug, no embedded frontend) — developer default
[group('global')]
build: frontend-build backend-build

# Build frontend then backend with embedded assets (release) — production build (CI/deployment)
[group('global')]
build-release: frontend-build backend-build-release

# Build + test release build (CI): run backend tests first for quick feedback,
# then compile release and validate with Playwright E2E
[group('global')]
test-release: backend-test-release build-release
    PICASU_BINARY=backend/target/release/picasu just frontend-playwright

# Remove the dev sandbox's generated app state (sandbox/data); leaves sandbox/images alone
[group('global')]
clean:
    rm -rf .testruns/*
    rm -rf sandbox/data

# Build (debug, no embedded frontend) and launch a clean instance against sandbox/{data,images}
[group('global')]
run: clean build
    #!/usr/bin/env sh
    set -e
    mkdir -p sandbox/images
    cd backend && \
        PICASU_CONFIG_HOME="{{justfile_directory()}}/sandbox/data" \
        PICASU_DATA_HOME="{{justfile_directory()}}/sandbox/data" \
        PICASU_IMAGE_HOME="{{justfile_directory()}}/sandbox/images" \
        cargo run --bin picasu

# Run security audits (backend + frontend)
[group('global')]
audit: backend-audit backend-deny frontend-audit

# Pre-commit check: run format + linter/static checks.
# On main, we enforce full tests as well. This is to support
# test-driven development in development branches.
# Developers know best which tests to fix and what to delegate to CI,
# but then again, these are safe defaults when people are in a hurry.
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
        just openapi-docs-check
        exit 0
    fi

    echo "[ precommit ] On '$branch' — format/lint enforced; run tests at your disgression."
    if echo "$changed" | grep -q '^backend/'; then
        just backend-check
    fi
    if echo "$changed" | grep -qE '^(\.plan/|docs/|[^/]+\.md$)'; then
        just plan-lint
        just docs-check
    fi
    if echo "$changed" | grep -q '^frontend/'; then
        just frontend-check
    fi
