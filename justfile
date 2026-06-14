# Urocissa dev tasks
# Install just: cargo install just
# Activate pre-commit hook: git config core.hooksPath .githooks

[private]
help:
    @just --list --unsorted

# ── Backend ────────────────────────────────────────────────────────────────────

# cargo fmt --check
[group('backend')]
backend-format:
    cd gallery-backend && cargo fmt --check

# cargo fmt
[group('backend')]
backend-format-fix:
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

# cargo build --release --features embed-frontend
[group('backend')]
backend-build:
    cd gallery-backend && cargo build --release --features embed-frontend

# ── Frontend ───────────────────────────────────────────────────────────────────

# prettier --check
[group('frontend')]
frontend-format:
    cd gallery-frontend && npx prettier --check .

# prettier --write
[group('frontend')]
frontend-format-fix:
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

# ── Global ─────────────────────────────────────────────────────────────────────

# Check formatting (backend + frontend)
[group('global')]
format: backend-format frontend-format

# Auto-fix formatting (backend + frontend)
[group('global')]
format-fix: backend-format-fix frontend-format-fix

# Run linters (backend + frontend)
[group('global')]
check: backend-check frontend-check

# Run tests (backend + frontend)
[group('global')]
test: backend-test frontend-test

# Install cargo dev tools (cargo-nextest, cargo-deny, cargo-audit)
[group('global')]
install-dev:
    cargo install cargo-nextest cargo-deny cargo-audit sccache

# Build frontend then backend with embedded assets (release)
[group('global')]
build: frontend-build backend-build

# Run security audits (backend + frontend)
[group('global')]
audit: backend-audit backend-deny frontend-audit

# Pre-commit check: format + check + test for each modified component
[group('global')]
precommit:
    #!/usr/bin/env sh
    set -e
    changed=$(git diff --cached --name-only)
    if echo "$changed" | grep -q '^gallery-backend/'; then
        just backend-format
        just backend-check
        just backend-test
    fi
    if echo "$changed" | grep -q '^gallery-frontend/'; then
        just frontend-format
        just frontend-check
        just frontend-test
    fi
