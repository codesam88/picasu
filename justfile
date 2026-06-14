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
    cd gallery-backend && cargo clippy -- -D warnings

# cargo test
[group('backend')]
backend-test:
    cd gallery-backend && cargo test --quiet

# cargo audit
[group('backend')]
backend-audit:
    cd gallery-backend && cargo audit

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

# (pending: add vitest)
[group('frontend')]
frontend-test:
    @echo "[ frontend-test ] no tests yet — add vitest"

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

# Run security audits (backend + frontend)
[group('global')]
audit: backend-audit frontend-audit

# Full pre-commit check: format + check + test
[group('global')]
precommit: format check test
