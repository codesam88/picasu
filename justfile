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

# cargo fmt --check + cargo clippy (all targets: lib, bins, tests, build script)
[group('backend')]
backend-check:
    cd backend && cargo fmt --check
    cargo clippy --all-targets -- -D warnings

[private]
nextest-present:
    #!/usr/bin/env sh
    command -v cargo-nextest >/dev/null || { echo "cargo-nextest is missing — run: just install-nextest" >&2; exit 1; }

# Backend tests (cargo-nextest)
[group('backend')]
backend-test: frontend-build-maybe nextest-present
    cd backend && PICASU_PBKDF_ITERATIONS="1000" cargo nextest run && cargo test --doc

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

# run frontend playwright tests, with optional filter
[group('frontend')]
frontend-playwright filter="": frontend-build-maybe
    #!/usr/bin/env bash
    set -e
    # Pre-build backend once so parallel test workers don't fight for the cargo build lock.
    # Skipped when PICASU_BINARY is already set (e.g. test-release supplies the release binary).
    [ -n "${PICASU_BINARY:-}" ] || cargo build --bin picasu
    export PICASU_BINARY="${PICASU_BINARY:-$(pwd)/target/debug/picasu}"
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

# npm run build (vue-tsc + vite build); installs deps if missing
[group('frontend')]
frontend-build:
    cd frontend && (test -d node_modules || npm ci) && npm run build

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

# cargo fmt --check + cargo clippy on utils/ crates (all targets)
[group('utils')]
utils-check:
    cargo fmt --check -p snapfab -p paste -p openapi-sanity
    cargo clippy --all-targets -p snapfab -p paste -p openapi-sanity -- -D warnings

# cargo test on utils/ crates
[group('utils')]
utils-test:
    cargo test -p snapfab -p openapi-sanity

# ── Tooling ─────────────────────────────────────────────────────────────────────

# Regenerate the checked-in OpenAPI artifact
[group('utils')]
openapi-gen:
    RUST_MIN_STACK=16777216 cargo run --package picasu -- --dump-openapi > backend/openapi.json
    @echo "wrote backend/openapi.json"

# Run all OpenAPI checks.
[group('utils')]
openapi-check: openapi-sanity openapi-json-match openapi-breakcheck openapi-lint openapi-routes-match

# Run dylint auth lints on the backend (advisory)
[group('utils')]
dylint-check:
    cargo dylint --all -- -p picasu

# Lint the OpenAPI document (errors fail)
[group('utils')]
openapi-lint:
    cd frontend && npx --no-install spectral lint --ruleset ../.spectral.yaml ../backend/openapi.json

# Check source-level OpenAPI annotation conventions.
[group('utils')]
openapi-sanity:
    cargo run --quiet -p openapi-sanity -- --source-root "{{justfile_directory()}}/backend/src/router" --app-error-map "{{justfile_directory()}}/backend/src/error.rs" --expect-at-least 60

# Compare mounted Rocket routes with documented OpenAPI operations.
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

# Pinned cargo-dylint release (matches the lint crate's dylint_linting; no sha256 — crates.io verification).
dylint_version := "6.1.0"

# Install pinned cargo-dylint (idempotent)
[group('utils')]
install-dylint:
    ./scripts/install-dylint.sh "{{ dylint_version }}"

# Pinned Schemathesis release for API fuzzing (via uvx; no sha256 — uv registry verification).
schemathesis_version := "4.30.1"

# Prefetch pinned Schemathesis (idempotent)
[group('utils')]
install-schemathesis:
    ./scripts/install-schemathesis.sh "{{ schemathesis_version }}"

# Fuzz a running seeded backend (advisory)
[group('utils')]
schemathesis-run *args:
    ./scripts/schemathesis-run.sh "{{ schemathesis_version }}" backend/openapi.json http://localhost:5673 {{ args }}

# Pinned oasdiff release for the breaking-change gate below.
# Upstream publishes checksums at
# https://github.com/oasdiff/oasdiff/releases/download/v<version>/checksums.txt
oasdiff_version := "1.33.0"
oasdiff_sha256 := "43a4e328e2d13ba1552d760aa68d2485c75c5621f309f6ff64ae895188345247"

# Install pinned oasdiff (idempotent)
[group('utils')]
install-oasdiff:
    ./scripts/install-oasdiff.sh "{{ oasdiff_version }}" "{{ oasdiff_sha256 }}"

# Fail on breaking OpenAPI changes
[group('utils')]
openapi-breakcheck:
    OASDIFF_SHA256="{{ oasdiff_sha256 }}" ./scripts/openapi-breakcheck.sh "{{ justfile_directory() }}" "{{ oasdiff_version }}"

# Auto-format .plan task frontmatter and body
[group('tooling')]
plan-format:
    cd frontend && npx --no-install prettier --write --no-error-on-unmatched-pattern '../.plan/**/*.md'

# check .plan task frontmatter correctness
[group('tooling')]
plan-lint:
    node {{justfile_directory()}}/scripts/plan-lint.mjs {{justfile_directory()}}

# run plan <args>, depends on `just install-plan`
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

# Format markdown files (README, docs, .plan, utils)
[group('docs')]
docs-format:
    cd frontend && npx --no-install prettier --write --no-error-on-unmatched-pattern '../*.md' '../docs/**/*.md' '../.plan/**/*.md' '../utils/**/*.md'

# Check markdown formatting (docs, .plan, utils)
[group('docs')]
docs-check:
    cd frontend && npx --no-install prettier --check --no-error-on-unmatched-pattern '../*.md' '../docs/**/*.md' '../.plan/**/*.md' '../utils/**/*.md'

# ── Global ─────────────────────────────────────────────────────────────────────

# Format everything
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

# Install 'plan' and 'gloss' tools (pinned)
[group('global')]
install-plan:
    cargo install --git https://github.com/tedsamhain/tablethat --rev 7853459

# Pinned ExifTool distribution for local development installs.
#
# ExifTool is snapfab's fixture metadata writer (`utils/snapfab`) and a
# dev/test prerequisite; the backend reads metadata in-process (kamadak-exif +
# xmpkit) and never spawns it. The distribution is pure Perl and unpacks
# without root, so a dev machine installs it into `~/.local`. CI installs the
# same pinned distribution (`just install-exiftool` in
# `.github/workflows/ci.yml`) because snapfab's tests spawn the binary and
# encode this version's refusal behaviour, which the distro package
# (`libimage-exiftool-perl`) does not necessarily reproduce.
#
# The version and its checksum move together: upstream publishes both at
# https://exiftool.org/checksums-<version>.txt, and the archive is fetched from
# the SourceForge mirror that exiftool.org's own download links point at.
exiftool_version := "13.59"
exiftool_sha256 := "668ea3acececb7235fbd0f4900e72d5f12c9b07e5c778fd36cb1e9b5828fd65a"

# Install pinned ExifTool (idempotent)
[group('global')]
install-exiftool:
    ./scripts/install-exiftool.sh "{{ exiftool_version }}" "{{ exiftool_sha256 }}"

# Pinned static ffmpeg build.
#
# The backend spawns ffmpeg (video thumbnails) and ffprobe (video metadata),
# and the release package ships the same static build. Pinning it here means
# the ffmpeg the tests run against is the ffmpeg that reaches the admin's
# machine, and that the digest is checked rather than whatever the URL
# serves today.
#
# The version and its digest move together: the tarball is fetched from the
# versioned release URL and verified against the digest below.
ffmpeg_version := "7.0.2"
ffmpeg_sha256 := "abda8d77ce8309141f83ab8edf0596834087c52467f6badf376a6a2a4c87cf67"

# Install pinned ffmpeg (idempotent)
[group('global')]
install-ffmpeg:
    ./scripts/install-ffmpeg.sh "{{ ffmpeg_version }}" "{{ ffmpeg_sha256 }}"

# Pinned cargo-nextest (the backend test runner).
#
# nextest runs one process per test, so the backend suite runs in parallel
# instead of serializing on the shared test data home. It publishes prebuilt
# binaries per platform, so the dev setup and CI install the binary rather
# than compiling it from source (`cargo install cargo-nextest` takes
# minutes).
#
# The version and its per-platform checksums move together: upstream
# publishes the binaries and a .sha256 sidecar per platform on the GitHub
# release.
nextest_version := "0.9.138"
nextest_sha256_linux_x86_64 := "3793bf0c27607b196f502c39b2108f571de89fcda7586ae6beefa11ee177b216"
nextest_sha256_linux_aarch64 := "e1c85dbe02dcf5b963803b3f96f4f561757f5c37cab68c932411dea20d57284e"
nextest_sha256_apple_darwin := "124baa34d160355fa8a8bed053856270cfcf132659e76a0bc8bbcdfc093266b7"

# Install pinned cargo-nextest (idempotent)
[group('global')]
install-nextest:
    ./scripts/install-nextest.sh "{{ nextest_version }}" "{{ nextest_sha256_linux_x86_64 }}" "{{ nextest_sha256_linux_aarch64 }}" "{{ nextest_sha256_apple_darwin }}"

# Pinned cargo-deny (the dependency audit).
#
# The audit job used to build it from source on every run
# (`cargo install cargo-deny` — minutes). cargo-deny publishes prebuilt
# binaries per platform, so the binary is installed instead.
#
# The version and its per-platform checksums move together: upstream
# publishes the binaries and a .sha256 sidecar per platform on the GitHub
# release.
cargo_deny_version := "0.20.2"
cargo_deny_sha256_linux_x86_64 := "9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"
cargo_deny_sha256_linux_aarch64 := "995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d"
cargo_deny_sha256_apple_x86_64 := "248da7f581724e470071990c088ffc55c811981715f4cbdb258621fb79f8b7a6"
cargo_deny_sha256_apple_aarch64 := "fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3"

# Install pinned cargo-deny (idempotent)
[group('global')]
install-cargo-deny:
    ./scripts/install-cargo-deny.sh "{{ cargo_deny_version }}" "{{ cargo_deny_sha256_linux_x86_64 }}" "{{ cargo_deny_sha256_linux_aarch64 }}" "{{ cargo_deny_sha256_apple_x86_64 }}" "{{ cargo_deny_sha256_apple_aarch64 }}"

# Install dev tools
[group('global')]
install-dev: install-nextest install-cargo-deny install-ffmpeg install-exiftool install-oasdiff install-dylint install-schemathesis
    npm ci --prefix frontend

# Build frontend then backend (dev build)
[group('global')]
build: frontend-build backend-build

# Release build with embedded assets
[group('global')]
build-release: frontend-build backend-build-release

# Build + test release build - webroot is embedded
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

# Build and run out of sandbox/{data,images}
[group('global')]
run: build
    #!/usr/bin/env sh
    set -e
    mkdir -p sandbox/images
    rm -rf sandbox/data
    # Dev-sandbox-only KDF effort (matches the e2e launcher); production default untouched.
    cd backend && \
        PICASU_BOOTSTRAP_ADMIN=true \
        PICASU_PBKDF_ITERATIONS=1000 \
        PICASU_CONFIG_HOME="{{justfile_directory()}}/sandbox/data" \
        PICASU_DATA_HOME="{{justfile_directory()}}/sandbox/data" \
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
    if echo "$changed" | grep -q '^utils/'; then
        just utils-check
    fi
    if echo "$changed" | grep -qE '^(backend|utils)/'; then
        just utils-test
    fi
    if echo "$changed" | grep -qE '^(\.plan/|docs/|[^/]+\.md$|utils/.*\.md$)'; then
        just plan-lint
        just docs-check
    fi
    if echo "$changed" | grep -q '^frontend/'; then
        just frontend-check
    fi
