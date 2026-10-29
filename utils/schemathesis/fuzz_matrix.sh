#!/usr/bin/env bash
# Execute the fuzz matrix against a throwaway seeded instance.
#
# Distinct from check.sh: that sweep asserts schema conformance and no 5xx,
# this one asserts the auth-model invariants per operation (cross-scope,
# wrong-typ, renewal binding, demoted token) that the campaign used to exercise
# only under a time budget. Both read the committed artifacts; neither carries
# a hand list.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

STATE_DIR="$(mktemp -d)"
export FUZZ_STATE_DIR="$STATE_DIR"
export PICASU_BINARY="${PICASU_BINARY:-$(pwd)/target/debug/picasu}"
export SNAPFAB_BINARY="${SNAPFAB_BINARY:-$(pwd)/target/debug/snapfab}"
export PICASU_WEB_ROOT="${PICASU_WEB_ROOT:-$(pwd)/frontend/dist}"
PORT="${PICASU_PORT:-8899}"
export PICASU_PORT="$PORT"

# Kill only the instance this run started.
cleanup() {
    rc=$?
    if [ -f "$STATE_DIR/server.pid" ]; then
        kill "$(cat "$STATE_DIR/server.pid")" 2>/dev/null || true
    fi
    rm -rf "$STATE_DIR"
    exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

cargo build --bin picasu
cargo build -p snapfab

utils/schemathesis/seed.sh
# shellcheck disable=SC1091
source "$STATE_DIR/seed.env"

python3 utils/schemathesis/fuzz_matrix.py "$REPO_ROOT"
