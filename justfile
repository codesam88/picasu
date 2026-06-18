# Urocissa dev tasks
# Install just: cargo install just
# Activate pre-commit hook: git config core.hooksPath .githooks

[private]
help:
    @just --list --unsorted

# ── Backend ────────────────────────────────────────────────────────────────────

# cargo fmt
[group('backend')]
backend-format:
    cd gallery-backend && cargo fmt

# cargo clippy -- -D warnings
[group('backend')]
backend-check:
    cd gallery-backend && cargo clippy -- -D warnings -A clippy::unwrap_used

# cargo nextest run
[group('backend')]
backend-test:
    cd gallery-backend && cargo nextest run

# cargo deny check
[group('backend')]
backend-deny:
    cd gallery-backend && cargo deny check

# cargo audit
[group('backend')]
backend-audit:
    cd gallery-backend && cargo audit

# cargo build (debug, no embedded frontend) — developer default; matches check/test
[group('backend')]
backend-build:
    cd gallery-backend && cargo build

# cargo build --release --features embed-frontend — production build (CI/deployment)
[group('backend')]
backend-build-release:
    cd gallery-backend && cargo build --release --features embed-frontend

# ── Frontend ───────────────────────────────────────────────────────────────────

# prettier --write
[group('frontend')]
frontend-format:
    cd gallery-frontend && npx prettier --write .

# vue-tsc + eslint
[group('frontend')]
frontend-check:
    cd gallery-frontend && npx vue-tsc --noEmit && npx eslint .

# vitest run
[group('frontend')]
frontend-test:
    cd gallery-frontend && npm test

# npm run build (npm ci + vue-tsc + vite build)
[group('frontend')]
frontend-build:
    cd gallery-frontend && npm run build

# npm audit
[group('frontend')]
frontend-audit:
    cd gallery-frontend && npm audit

# ── Spec tooling ───────────────────────────────────────────────────────────────

# Emit openapi.json from utoipa annotations (3 spike endpoints first)
[group('spec')]
emit-openapi:
    cargo xtask emit-openapi

# Generate scenario test code from YAML specs and run
[group('spec')]
test-backend:
    cargo xtask test-backend

# Generate scenario test code without running tests
[group('spec')]
gen:
    cargo xtask test-backend --generate-only

# Generate and run negative self-tests for the assertion pipeline
[group('spec')]
test-generator:
    cargo xtask test-generator

# Verify generated code is in sync with YAML sources (for CI / precommit)
[group('spec')]
check-generated:
    cargo xtask test-backend --generate-only
    git diff --exit-code -- gallery-backend/src/tests/scenarios_generated.rs

# ── Global ─────────────────────────────────────────────────────────────────────

# Format everything (backend + frontend + markdown + .plan)
[group('global')]
format: backend-format frontend-format
    npx prettier --write '*.md' 'docs/**/*.md' '.plan/**/*.md'
    cargo xtask plan --format

# Run linters (backend + frontend)
[group('global')]
check: backend-check frontend-check
    cargo xtask plan --lint

# Run tests (backend + frontend)
[group('global')]
test: backend-test frontend-test
    cargo test -p xtask

# Install all dev tooling (cargo tools + frontend deps including prettier)
[group('global')]
install-dev:
    cargo install sccache
    cargo install cargo-deny cargo-audit
    cargo install --locked cargo-nextest
    npm ci --prefix gallery-frontend

# Build frontend then backend (debug, no embedded frontend) — developer default
[group('global')]
build: frontend-build backend-build

# Build frontend then backend with embedded assets (release) — production build (CI/deployment)
[group('global')]
build-release: frontend-build backend-build-release

# Remove the dev sandbox's generated app state (sandbox/data); leaves sandbox/images alone
[group('global')]
clean:
    rm -rf sandbox/data

# Build (debug, no embedded frontend) and launch a clean instance against sandbox/{data,images}
[group('global')]
run: clean build
    #!/usr/bin/env sh
    set -e
    mkdir -p sandbox/images
    cd gallery-backend && \
        UROCISSA_CONFIG_HOME="{{justfile_directory()}}/sandbox/data" \
        UROCISSA_DATA_HOME="{{justfile_directory()}}/sandbox/data" \
        UROCISSA_IMAGE_HOME="{{justfile_directory()}}/sandbox/images" \
        cargo run

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
        just format
        just check
        just test
        just check-generated
        exit 0
    fi

    echo "[ precommit ] On '$branch' — format/lint enforced; run tests at your disgression."
    if echo "$changed" | grep -q '^gallery-backend/'; then
        cd gallery-backend && cargo fmt
        just backend-check
    fi
    if echo "$changed" | grep -qE '^(\.plan/|docs/|[^/]+\.md$)'; then
        cargo xtask plan --format
        npx prettier --write '*.md' 'docs/**/*.md' '.plan/**/*.md'
    fi
    if echo "$changed" | grep -qE '^(xtask/data|xtask/src|gallery-backend/src/tests/scenarios_generated)'; then
        just check-generated
    fi
    if echo "$changed" | grep -q '^gallery-frontend/'; then
        npx prettier --write gallery-frontend/
        just frontend-check
    fi
