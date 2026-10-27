#!/usr/bin/env bash
# Tier-2 Schemathesis campaign: stateful run against a running backend.
# Usage: campaign.sh <schemathesis-version> [schemathesis args...]
#        (run from the repository root)
#
# Advisory: the exit code is the baseline comparison — known (accepted)
# findings do not fail the run, new ones do, and a crashed run exits with
# Schemathesis's own code before comparing anything.
# Prerequisites: a backend on the target URL (seed one per the README for
# probes and authenticated runs); PICASU_JWT and the fixture ids come from
# seed.env when FUZZ_STATE_DIR points at a seed directory.
set -euo pipefail

version="${1:?usage: campaign.sh <schemathesis-version> [args...]}"
shift

export PATH="$HOME/.local/bin:$PATH"
cd "$(dirname "$0")/../.."

if [ -n "${FUZZ_STATE_DIR:-}" ] && [ -f "${FUZZ_STATE_DIR}/seed.env" ]; then
    # shellcheck disable=SC1091
    source "${FUZZ_STATE_DIR}/seed.env"
fi
export FUZZ_SHARE_HEADERS="${FUZZ_SHARE_HEADERS:-1}"
export FUZZ_MARKERS="${FUZZ_MARKERS:-1}"
BASE_URL="${PICASU_BASE_URL:-http://localhost:5673}"
export PICASU_BASE_URL="$BASE_URL"

# Fail with the reason instead of a Schemathesis connection trace.
if ! curl -s -o /dev/null --max-time 5 "$BASE_URL/" 2>/dev/null; then
    echo "campaign: no backend answering at $BASE_URL" >&2
    echo "  start one with 'just run', or seed a disposable instance (see utils/schemathesis/README.md)" >&2
    exit 2
fi

mkdir -p utils/schemathesis/out
REPORT="utils/schemathesis/out/report.json"
rm -f "$REPORT" # a stale report must never be compared after a failed run

# Default budget; a pass-through --max-time wins.
if [[ " $* " == *" --max-time "* ]]; then
    time_args=("$@")
else
    time_args=(--max-time 900 "$@")
fi

set +e
uvx schemathesis@"$version" --config-file utils/schemathesis/campaign.toml --no-color \
    run backend/openapi.json --url "$BASE_URL" \
    "${time_args[@]}" \
    --phases examples,coverage,fuzzing,stateful -n 8 --max-redirects 0 \
    --checks not_a_server_error,status_code_conformance,content_type_conformance,response_schema_conformance,response_headers_conformance \
    --report-json-path "$REPORT"
st_rc=$?
set -e

if [ $st_rc -ne 0 ] && [ $st_rc -ne 1 ]; then
    echo "schemathesis exited with unexpected code $st_rc" >&2
    exit $st_rc
fi

echo "=== baseline compare ==="
set +e
python3 utils/schemathesis/check_report.py "$REPORT" utils/schemathesis/baseline.yaml
cmp_rc=$?
set -e
exit $cmp_rc
