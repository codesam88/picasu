#!/usr/bin/env bash
# Fail on breaking OpenAPI changes vs the base branch's committed spec.
#
# Compares a freshly generated spec (not the committed file — drift between
# the two is openapi-json-match's job) against the base branch's committed
# backend/openapi.json. Breaking findings fail; entries in
# .oasdiff-ignore.txt are explicitly accepted breaks, not failures.
#
# Usage: openapi-breakcheck.sh <repo-root> <oasdiff-version>
# Baseline: $OASDIFF_BASE_BRANCH, else $GITHUB_BASE_REF (PRs), else `main`.
# CI checks out with full history so origin/<branch> resolves locally; on
# shallow clones the base is fetched here (the only network use) instead of
# touching the shared checkout config.
set -euo pipefail

repo_root="${1:?usage: openapi-breakcheck.sh <repo-root> <oasdiff-version>}"
oasdiff_version="${2:?usage: openapi-breakcheck.sh <repo-root> <oasdiff-version>}"
cd "$repo_root"

if ! command -v oasdiff >/dev/null 2>&1 \
    || [ "$(oasdiff --version 2>/dev/null || true)" != "oasdiff version $oasdiff_version" ]; then
    ./scripts/install-oasdiff.sh "$oasdiff_version" "$OASDIFF_SHA256"
    export PATH="$HOME/.local/bin:$PATH"
fi

branch="${OASDIFF_BASE_BRANCH:-${GITHUB_BASE_REF:-main}}"
if git rev-parse --verify --quiet "origin/$branch^{commit}" >/dev/null; then
    base="origin/$branch"
elif git rev-parse --verify --quiet "$branch^{commit}" >/dev/null; then
    base="$branch"
else
    echo "→ base ref '$branch' not present locally; fetching from origin"
    git fetch origin "$branch" --depth=1
    base="origin/$branch"
fi
if ! git cat-file -e "$base:backend/openapi.json" 2>/dev/null; then
    echo "base ref '$base' has no committed backend/openapi.json; cannot diff" >&2
    exit 1
fi

baseline="$(mktemp)"
generated="$(mktemp)"
trap 'rm -f "$baseline" "$generated"' EXIT
git show "$base:backend/openapi.json" > "$baseline"
RUST_MIN_STACK=16777216 cargo run -q --package picasu -- --dump-openapi > "$generated"

# --include-path-params: the spec has sibling template-only routes
# (/{dynamic_album_id} and /{path}) that oasdiff's default endpoint
# matching treats as duplicates; matching on param names keeps them apart.
# --fail-on ERR: without it oasdiff prints errors yet exits 0.
if oasdiff breaking --include-path-params --fail-on ERR \
    --err-ignore "$repo_root/.oasdiff-ignore.txt" \
    "$baseline" "$generated"; then
    echo "no breaking OpenAPI changes vs $base"
else
    echo ""
    echo "--- oasdiff summary ---"
    oasdiff summary --include-path-params "$baseline" "$generated" || true
    echo ""
    echo "Breaking OpenAPI changes detected vs $base."
    echo "If a finding is intended, record it in .oasdiff-ignore.txt; otherwise fix the annotations."
    exit 1
fi
