#!/usr/bin/env bash
# Per-role Schemathesis runner: one deterministic pass per role, equal budget.
#
# Runs admin0, admin1, memberA, memberB, sharingDisabled, disabledUser,
# share-guest, snapshot-bearer, and anonymous with IDENTICAL max-examples /
# deadline / seed. Each role gets its own report-<role>.json.
#
# Usage: run-roles.sh [<schemathesis-version>]
# Env: FUZZ_STATE_DIR (default: temp dir), FUZZ_REPORT_DIR (default:
#      utils/schemathesis/out), MAX_EXAMPLES, DEADLINE, SEED.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

# shellcheck disable=SC1091
source "$SCRIPT_DIR/../../scripts/deps/versions.env"
if [[ $# -gt 0 && "$1" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
    version="$1"
    shift
else
    version="$SCHEMATHESIS_VERSION"
fi
: "${version:?usage: run-roles.sh <schemathesis-version>}"

export PATH="$HOME/.local/bin:$PATH"

# Reduced (test/debug) budget. Full-budget values for later full runs:
#   MAX_EXAMPLES=25
#   DEADLINE=120
MAX_EXAMPLES="${MAX_EXAMPLES:-3}"
DEADLINE="${DEADLINE:-15}"
SEED="${SEED:-42}"

STATE_DIR_INPUT="${FUZZ_STATE_DIR:-}"
if [ -z "$STATE_DIR_INPUT" ]; then
    STATE_DIR="$(mktemp -d)"
else
    STATE_DIR="$STATE_DIR_INPUT"
    mkdir -p "$STATE_DIR"
fi
export FUZZ_STATE_DIR="$STATE_DIR"

REPORT_DIR="${FUZZ_REPORT_DIR:-$REPO_ROOT/utils/schemathesis/out}"
mkdir -p "$REPORT_DIR"
# Clear stale per-role artifacts.
rm -f "$REPORT_DIR"/report-*.json "$REPORT_DIR"/role-*.log

export PICASU_BINARY="${PICASU_BINARY:-$REPO_ROOT/target/debug/picasu}"
export SNAPFAB_BINARY="${SNAPFAB_BINARY:-$REPO_ROOT/target/debug/snapfab}"
export PICASU_WEB_ROOT="${PICASU_WEB_ROOT:-$REPO_ROOT/frontend/dist}"
PORT="${PICASU_PORT:-8899}"
export PICASU_PORT="$PORT"
BASE_URL="http://127.0.0.1:$PORT"

cargo build --bin picasu >/dev/null
cargo build -p snapfab >/dev/null

cleanup() {
    rc=$?
    if [ -f "$STATE_DIR/server.pid" ]; then
        kill "$(cat "$STATE_DIR/server.pid")" 2>/dev/null || true
    fi
    exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

utils/schemathesis/seed.sh
# shellcheck disable=SC1091
source "$STATE_DIR/seed.env"
export PICASU_BASE_URL="$BASE_URL"

# Mint a snapshot-bearer token from share A.
snapshot_resp="$(curl -s -X POST "$BASE_URL/get/prefetch" \
    -H "x-album-id: $FUZZ_ALBUM_A" -H "x-share-id: $FUZZ_SHARE_A" \
    -H 'Content-Type: application/json' -d '{}')"
FUZZ_SNAPSHOT_TOKEN="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])' <<<"$snapshot_resp")"
[ -n "$FUZZ_SNAPSHOT_TOKEN" ] || { echo "failed to mint snapshot token" >&2; exit 1; }
export FUZZ_SNAPSHOT_TOKEN

run_role() {
    local role="$1"
    local jwt="$2"
    local share_headers="$3"   # 1/0
    local snapshot="$4"        # 1/0
    local log="$REPORT_DIR/role-$role.log"
    local report="$REPORT_DIR/report-$role.json"

    # Reset per-role auth env.
    if [ "$jwt" = "__ANON__" ]; then
        unset PICASU_JWT
    else
        export PICASU_JWT="$jwt"
    fi
    if [ "$snapshot" = "1" ]; then
        : # FUZZ_SNAPSHOT_TOKEN already set
    else
        unset FUZZ_SNAPSHOT_TOKEN
    fi
    export FUZZ_SHARE_HEADERS="$share_headers"

    # For snapshot role, FUZZ_SNAPSHOT_TOKEN must be present.
    if [ "$snapshot" = "1" ]; then
        export FUZZ_SNAPSHOT_TOKEN="$FUZZ_SNAPSHOT_TOKEN_SAVED"
    fi

    set +e
    uvx schemathesis@"$version" --config-file utils/schemathesis/roles.toml --no-color \
        run backend/openapi.json --url "$BASE_URL" \
        --phases examples,coverage,fuzzing -n "$MAX_EXAMPLES" --max-redirects 0 \
        --max-time "$DEADLINE" --seed "$SEED" \
        --checks not_a_server_error,status_code_conformance,content_type_conformance,response_schema_conformance,response_headers_conformance \
        --report-json-path "$report" >"$log" 2>&1
    st_rc=$?
    set -e

    # Abort on rate/limit errors.
    if grep -Eqi '429|rate[ -]?limit|too many requests' "$log" "$report" 2>/dev/null; then
        echo "BLOCKED: rate/limit error detected for role $role" >&2
        cat "$log" >&2
        exit 1
    fi
    if [ "$st_rc" -ne 0 ] && [ "$st_rc" -ne 1 ]; then
        echo "schemathesis exited with unexpected code $st_rc for role $role" >&2
        cat "$log" >&2
        exit "$st_rc"
    fi
    echo "role $role done (exit $st_rc)"
}

# Save snapshot token for the snapshot role (run_role unsets for others).
FUZZ_SNAPSHOT_TOKEN_SAVED="$FUZZ_SNAPSHOT_TOKEN"

# Role matrix: role, jwt, share_headers, snapshot
run_role "admin0" "$PICASU_JWT" 1 0
run_role "admin1" "$PICASU_JWT_ADMIN1" 1 0
run_role "memberA" "$PICASU_JWT_MEMBER_A" 1 0
run_role "memberB" "$PICASU_JWT_MEMBER_B" 1 0
run_role "sharingDisabled" "$PICASU_JWT_SHARING_DISABLED" 1 0
run_role "disabledUser" "$PICASU_JWT_DISABLED" 1 0
run_role "shareGuest" "__ANON__" 1 0
run_role "snapshotBearer" "__ANON__" 0 1
run_role "anonymous" "__ANON__" 0 0

echo "=== per-role summary ==="
for r in admin0 admin1 memberA memberB sharingDisabled disabledUser shareGuest snapshotBearer anonymous; do
    rep="$REPORT_DIR/report-$r.json"
    if [ -f "$rep" ]; then
        gen="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["test_cases"]["generated"])' "$rep" 2>/dev/null || echo "?")"
        echo "$r: generated=$gen"
    else
        echo "$r: MISSING REPORT"
    fi
done