#!/usr/bin/env bash
# Tier-1 Schemathesis check: seed a throwaway backend, run the bounded
# single-op sweep, compare findings against the accepted-findings baseline.
# Usage: check.sh <schemathesis-version>   (run from the repository root)
#
# Exits 0 iff the tool ran and every finding is baseline-accepted. The
# instance this run starts is torn down on exit (pid-scoped — never a
# dev server elsewhere); FUZZ_REPORT_OUT copies the JSON report out first.
set -euo pipefail

version="${1:?usage: check.sh <schemathesis-version>}"
export PATH="$HOME/.local/bin:$PATH"

cd "$(dirname "$0")/../.."

cargo build --bin picasu
cargo build -p snapfab

STATE_DIR="$(mktemp -d)"
export FUZZ_STATE_DIR="$STATE_DIR"
export PICASU_BINARY="${PICASU_BINARY:-$(pwd)/target/debug/picasu}"
export SNAPFAB_BINARY="${SNAPFAB_BINARY:-$(pwd)/target/debug/snapfab}"
export PICASU_WEB_ROOT="${PICASU_WEB_ROOT:-$(pwd)/frontend/dist}"
PORT="${PICASU_PORT:-8899}"
export PICASU_PORT="$PORT"
BASE_URL="http://127.0.0.1:$PORT"

# Kill only the instance this run started (never a dev server elsewhere);
# INT/TERM route through exit so the EXIT trap always tears down.
cleanup() {
    rc=$?
    if [ -n "${FUZZ_REPORT_OUT:-}" ] && [ -f "${STATE_DIR}/report.json" ]; then
        mkdir -p "$(dirname "$FUZZ_REPORT_OUT")"
        cp "${STATE_DIR}/report.json" "$FUZZ_REPORT_OUT"
    fi
    if [ -f "$STATE_DIR/server.pid" ]; then
        kill "$(cat "$STATE_DIR/server.pid")" 2>/dev/null || true
    fi
    rm -rf "$STATE_DIR"
    exit $rc
}
trap cleanup EXIT
trap 'exit 130' INT TERM

utils/schemathesis/seed.sh
# shellcheck disable=SC1091
source "$STATE_DIR/seed.env"
export PICASU_BASE_URL="$BASE_URL"
REPORT="$STATE_DIR/report.json"

# --max-redirects 0: judge declared redirect statuses directly instead of
# following them to the SPA shell and grading that instead.
set +e
uvx schemathesis@"$version" --config-file utils/schemathesis/check.toml --no-color \
    run backend/openapi.json --url "$BASE_URL" \
    --phases examples,coverage,fuzzing -n 25 --max-redirects 0 \
    --checks not_a_server_error,status_code_conformance,content_type_conformance,response_schema_conformance,response_headers_conformance \
    --report-json-path "$REPORT"
st_rc=$?
python3 utils/schemathesis/check_report.py "$REPORT" utils/schemathesis/baseline.yaml
cmp_rc=$?
set -e
if [ $st_rc -ne 0 ] && [ $st_rc -ne 1 ]; then
    echo "schemathesis exited with unexpected code $st_rc" >&2
    exit $st_rc
fi
exit $cmp_rc
