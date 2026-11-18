#!/usr/bin/env bash
# Unified fuzz-harness entry point. Prefer `just fuzz <verb>` from the repo
# root: the justfile installs the pinned tool and builds frontend assets.
#
# Usage: fuzz.sh <verb> [options] [schemathesis args...]
#
# Verbs:
#   check   Tier-1 A1-A5 sweep, compared to baseline.yaml (gating)
#   matrix  Deterministic auth-matrix checks (gating)
#   roles   One deterministic pass per role with an equal budget
#   run     Advisory stateful campaign, seeded on first use (FUZZ_MIN minutes)
#   diff    Differential diff across per-role reports
#
# Options:
#   --level smoke|min|full  Budget preset (roles default smoke, run default full)
#   --max-examples N        Generation examples per operation (roles)
#   --deadline SECONDS      Per-role time limit (roles)
#   --seed N                Generation seed (roles, default 42)
#   --min MINUTES           Campaign time budget (run)
#   --state-dir DIR         Instance state directory (roles, run)
#   --report-dir DIR        Per-role report directory (roles)
#   --base-url URL          Existing backend to target (run)
#   -h, --help              This text
#
# Environment defaults and the seeded contract: utils/fuzz/README.md.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck disable=SC1091
source "$REPO_ROOT/scripts/deps/versions.env"
VERSION="${SCHEMATHESIS_VERSION:?missing SCHEMATHESIS_VERSION in scripts/deps/versions.env}"
export PATH="$HOME/.local/bin:$PATH"
# Recipes run in the module directory; every relative path below (config files,
# report dirs, state dirs) is repo-relative.
cd "$REPO_ROOT"

# Judge declared redirect statuses directly instead of following them to the
# SPA shell and grading that instead.
CHECKS='not_a_server_error,status_code_conformance,content_type_conformance,response_schema_conformance,response_headers_conformance'
PHASES='examples,coverage,fuzzing'
ROLES=(admin0 admin1 memberA memberB sharingDisabled disabledUser shareGuest snapshotBearer anonymous)

STATE_DIR=""
THROWAWAY_DIR=""
SERVER_PID=""

die() {
    echo "fuzz: $*" >&2
    exit 2
}

usage() {
    sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p;}' "${BASH_SOURCE[0]}"
}

# Remove only what this run created: the instance it started and a throwaway
# state directory. A served backend the caller supplied is left running.
stop_pid() {
    local pid="$1"
    [ -n "$pid" ] || return 0
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 10); do
        kill -0 "$pid" 2>/dev/null || return 0
        sleep 0.5
    done
    kill -9 "$pid" 2>/dev/null || true
}

cleanup() {
    rc=$?
    if [ -n "${FUZZ_REPORT_OUT:-}" ] && [ -n "$STATE_DIR" ] && [ -f "$STATE_DIR/report.json" ]; then
        mkdir -p "$(dirname "$FUZZ_REPORT_OUT")"
        cp "$STATE_DIR/report.json" "$FUZZ_REPORT_OUT"
    fi
    if [ -n "$SERVER_PID" ]; then
        stop_pid "$SERVER_PID"
    fi
    if [ -n "$THROWAWAY_DIR" ]; then
        rm -rf "$THROWAWAY_DIR"
    fi
    exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

level_values() {
    case "$1" in
        smoke) LEVEL_MAX_EXAMPLES=3; LEVEL_DEADLINE=15; LEVEL_MINUTES=1 ;;
        min) LEVEL_MAX_EXAMPLES=10; LEVEL_DEADLINE=60; LEVEL_MINUTES=5 ;;
        full) LEVEL_MAX_EXAMPLES=25; LEVEL_DEADLINE=120; LEVEL_MINUTES=15 ;;
        *) die "unknown level '$1' (expected smoke, min, or full)" ;;
    esac
}

prepare_instance_env() {
    export PICASU_BINARY="${PICASU_BINARY:-$REPO_ROOT/target/debug/picasu}"
    export SNAPFAB_BINARY="${SNAPFAB_BINARY:-$REPO_ROOT/target/debug/snapfab}"
    export PICASU_WEB_ROOT="${PICASU_WEB_ROOT:-$REPO_ROOT/frontend/dist}"
    export PICASU_PORT="${PICASU_PORT:-8899}"
}

build_binaries() {
    (cd "$REPO_ROOT" && cargo build --bin picasu && cargo build -p snapfab)
}

wait_for_server() {
    local base="$1" code
    for _ in $(seq 1 60); do
        code="$(curl -s -o /dev/null -w '%{http_code}' "$base/" || true)"
        [ "$code" = "200" ] && return 0
        sleep 1
    done
    die "backend did not answer at $base"
}

# State directory for a verb: a caller-supplied one is reused (seed.sh reseeds
# it), otherwise a throwaway directory is created and removed on exit.
prepare_state_dir() {
    local dir="${FUZZ_STATE_DIR:-}"
    if [ -n "$dir" ]; then
        mkdir -p "$dir"
    else
        THROWAWAY_DIR="$(mktemp -d)"
        dir="$THROWAWAY_DIR"
    fi
    STATE_DIR="$dir"
}

# Boot a background backend from an existing seeded state directory. Used by
# `run` so repeated campaigns share one seeded corpus instead of reseeding.
boot_seeded_server() {
    local dir="$1"
    if [ -f "$dir/server.pid" ]; then
        stop_pid "$(cat "$dir/server.pid")"
        rm -f "$dir/server.pid"
    fi
    (
        cd "$dir"
        env PICASU_PORT="$PICASU_PORT" \
            PICASU_DATA_HOME="$dir/data" \
            PICASU_CONFIG_HOME="$dir/config" \
            PICASU_IMAGE_HOME="$dir/images" \
            PICASU_USERS_HOME="$dir/data/users" \
            PICASU_WEB_ROOT="$PICASU_WEB_ROOT" \
            PICASU_AUTH_KEY='fuzz-auth-key-0123456789abcdef' \
            PICASU_RATE_LIMITS='false' \
            PICASU_BOOTSTRAP_ADMIN='false' \
            nohup "$PICASU_BINARY" >"$dir/server.log" 2>&1 &
        echo $! >"$dir/server.pid"
    )
    SERVER_PID="$(cat "$dir/server.pid")"
    wait_for_server "$PICASU_BASE_URL"
}

# Seed a fresh instance into the state directory and adopt its environment.
seed_instance() {
    local dir="$1"
    export FUZZ_STATE_DIR="$dir"
    prepare_instance_env
    build_binaries
    "$SCRIPT_DIR/seed.sh"
    SERVER_PID="$(cat "$dir/server.pid" 2>/dev/null || true)"
    # shellcheck disable=SC1091
    source "$dir/seed.env"
    export PICASU_BASE_URL="http://127.0.0.1:$PICASU_PORT"
}

maybe_login() {
    [ -n "${PICASU_JWT:-}" ] && return 0
    [ -n "${FUZZ_ADMIN_PASSWORD:-}" ] || return 0
    local admin_user="${FUZZ_ADMIN_USER:-admin}" headers body code token
    headers="$(mktemp)"
    body="$(mktemp)"
    code="$(curl -s -D "$headers" -o "$body" -w '%{http_code}' \
        -X POST "$PICASU_BASE_URL/post/users/authenticate" \
        -H 'Content-Type: application/json' \
        -d "{\"userId\":\"$admin_user\",\"password\":\"$FUZZ_ADMIN_PASSWORD\"}")"
    token="$(sed -n 's/^set-cookie: jwt=\([^;]*\).*/\1/pI' "$headers" | tr -d '\r')"
    rm -f "$headers" "$body"
    if [ "$code" != "200" ] || [ -z "$token" ]; then
        echo "fuzz: login failed for user '$admin_user' at $PICASU_BASE_URL (HTTP $code)" >&2
        exit 1
    fi
    export PICASU_JWT="$token"
}

cmd_check() {
    local args=()
    while [ $# -gt 0 ]; do
        case "$1" in
            -h | --help) usage; exit 0 ;;
            *) args+=("$1"); shift ;;
        esac
    done

    prepare_state_dir
    seed_instance "$STATE_DIR"
    local report="$STATE_DIR/report.json" st_rc=0

    set +e
    uvx schemathesis@"$VERSION" --config-file utils/fuzz/check.toml --no-color \
        run backend/openapi.json --url "$PICASU_BASE_URL" \
        --phases "$PHASES" -n 25 --max-redirects 0 \
        --checks "$CHECKS" --report-json-path "$report" "${args[@]}"
    st_rc=$?
    set -e
    if [ "$st_rc" -ne 0 ] && [ "$st_rc" -ne 1 ]; then
        die "schemathesis exited with unexpected code $st_rc"
    fi
    python3 utils/fuzz/check_report.py "$report" utils/fuzz/baseline.yaml
}

cmd_matrix() {
    if [ $# -gt 0 ]; then
        case "$1" in
            -h | --help) usage; exit 0 ;;
            *) die "matrix takes no arguments (got '$1')" ;;
        esac
    fi
    prepare_state_dir
    seed_instance "$STATE_DIR"
    python3 utils/fuzz/fuzz_matrix.py "$REPO_ROOT"
}

cmd_roles() {
    local level="${FUZZ_LEVEL:-smoke}" max="${MAX_EXAMPLES:-}" deadline="${DEADLINE:-}"
    local seed="${SEED:-42}" report_dir="${FUZZ_REPORT_DIR:-$SCRIPT_DIR/out}"
    while [ $# -gt 0 ]; do
        case "$1" in
            --level) level="${2:?--level needs a value}"; shift 2 ;;
            --max-examples) max="${2:?--max-examples needs a value}"; shift 2 ;;
            --deadline) deadline="${2:?--deadline needs a value}"; shift 2 ;;
            --seed) seed="${2:?--seed needs a value}"; shift 2 ;;
            --report-dir) report_dir="${2:?--report-dir needs a value}"; shift 2 ;;
            --state-dir) export FUZZ_STATE_DIR="${2:?--state-dir needs a value}"; shift 2 ;;
            -h | --help) usage; exit 0 ;;
            *) die "unknown option for roles: $1" ;;
        esac
    done
    level_values "$level"
    max="${max:-$LEVEL_MAX_EXAMPLES}"
    deadline="${deadline:-$LEVEL_DEADLINE}"

    prepare_state_dir
    seed_instance "$STATE_DIR"
    mkdir -p "$report_dir"
    rm -f "$report_dir"/report-*.json "$report_dir"/role-*.log

    local snapshot_token snapshot_resp
    snapshot_resp="$(curl -s -X POST "$PICASU_BASE_URL/get/prefetch" \
        -H "x-album-id: $FUZZ_ALBUM_A" -H "x-share-id: $FUZZ_SHARE_A" \
        -H 'Content-Type: application/json' -d '{}')"
    snapshot_token="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])' <<<"$snapshot_resp")"
    [ -n "$snapshot_token" ] || die "failed to mint snapshot token"

    echo "roles: level=$level max-examples=$max deadline=${deadline}s seed=$seed"
    for role in "${ROLES[@]}"; do
        run_role "$role" "$snapshot_token" "$max" "$deadline" "$seed" "$report_dir"
    done

    echo "=== per-role summary ==="
    local report generated
    for role in "${ROLES[@]}"; do
        report="$report_dir/report-$role.json"
        if [ -f "$report" ]; then
            generated="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["test_cases"]["generated"])' "$report" 2>/dev/null || echo "?")"
            echo "$role: generated=$generated"
        else
            echo "$role: MISSING REPORT"
        fi
    done
}

# One role membership: JWT and credential mode, applied to the hook env.
role_credentials() {
    local role="$1"
    case "$role" in
        admin0) ROLE_JWT="${PICASU_JWT:-}" ROLE_SHARE_HEADERS=1 ROLE_SNAPSHOT=0 ;;
        admin1) ROLE_JWT="${PICASU_JWT_ADMIN1:-}" ROLE_SHARE_HEADERS=1 ROLE_SNAPSHOT=0 ;;
        memberA) ROLE_JWT="${PICASU_JWT_MEMBER_A:-}" ROLE_SHARE_HEADERS=1 ROLE_SNAPSHOT=0 ;;
        memberB) ROLE_JWT="${PICASU_JWT_MEMBER_B:-}" ROLE_SHARE_HEADERS=1 ROLE_SNAPSHOT=0 ;;
        sharingDisabled) ROLE_JWT="${PICASU_JWT_SHARING_DISABLED:-}" ROLE_SHARE_HEADERS=1 ROLE_SNAPSHOT=0 ;;
        disabledUser) ROLE_JWT="${PICASU_JWT_DISABLED:-}" ROLE_SHARE_HEADERS=1 ROLE_SNAPSHOT=0 ;;
        shareGuest) ROLE_JWT="" ROLE_SHARE_HEADERS=1 ROLE_SNAPSHOT=0 ;;
        snapshotBearer) ROLE_JWT="" ROLE_SHARE_HEADERS=0 ROLE_SNAPSHOT=1 ;;
        anonymous) ROLE_JWT="" ROLE_SHARE_HEADERS=0 ROLE_SNAPSHOT=0 ;;
        *) die "unknown role '$role'" ;;
    esac
}

run_role() {
    local role="$1" snapshot_token="$2" max="$3" deadline="$4" seed="$5" report_dir="$6"
    role_credentials "$role"

    if [ -n "$ROLE_JWT" ]; then
        export PICASU_JWT="$ROLE_JWT"
    else
        unset PICASU_JWT
    fi
    if [ "$ROLE_SNAPSHOT" = "1" ]; then
        export FUZZ_SNAPSHOT_TOKEN="$snapshot_token"
    else
        unset FUZZ_SNAPSHOT_TOKEN
    fi
    export FUZZ_SHARE_HEADERS="$ROLE_SHARE_HEADERS"

    local log="$report_dir/role-$role.log"
    local report="$report_dir/report-$role.json"
    local st_rc=0
    set +e
    uvx schemathesis@"$VERSION" --config-file utils/fuzz/roles.toml --no-color \
        run backend/openapi.json --url "$PICASU_BASE_URL" \
        --phases "$PHASES" -n "$max" --max-redirects 0 \
        --max-time "$deadline" --seed "$seed" \
        --checks "$CHECKS" --report-json-path "$report" >"$log" 2>&1
    st_rc=$?
    set -e

    # Rate limiting invalidates the comparison, so it aborts the whole run.
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

cmd_run() {
    local level="${FUZZ_LEVEL:-}" minutes="${FUZZ_MIN:-}"
    local state_dir="${FUZZ_STATE_DIR:-$REPO_ROOT/sandbox/fuzz}" base_url="${PICASU_BASE_URL:-}"
    local explicit_state=0
    if [ -n "${FUZZ_STATE_DIR:-}" ]; then explicit_state=1; fi
    local args=()
    while [ $# -gt 0 ]; do
        case "$1" in
            --level) level="${2:?--level needs a value}"; shift 2 ;;
            --min) minutes="${2:?--min needs a value}"; shift 2 ;;
            --state-dir) state_dir="${2:?--state-dir needs a value}"; explicit_state=1; shift 2 ;;
            --base-url) base_url="${2:?--base-url needs a value}"; shift 2 ;;
            -h | --help) usage; exit 0 ;;
            *) args+=("$1"); shift ;;
        esac
    done
    if [ -n "$level" ]; then
        level_values "$level"
        minutes="${minutes:-$LEVEL_MINUTES}"
    fi
    minutes="${minutes:-15}"

    if [ -n "$base_url" ]; then
        export PICASU_BASE_URL="$base_url"
        # Credentials come from a seed directory only when the caller named it;
        # a bare URL must not pick up another target's seed.env.
        if [ "$explicit_state" = 1 ] && [ -f "$state_dir/seed.env" ]; then
            # shellcheck disable=SC1091
            source "$state_dir/seed.env"
            export PICASU_BASE_URL="$base_url"
        fi
        if ! curl -s -o /dev/null --max-time 5 "$PICASU_BASE_URL/" 2>/dev/null; then
            echo "fuzz: no backend answering at $PICASU_BASE_URL" >&2
            exit 2
        fi
    else
        mkdir -p "$state_dir"
        export FUZZ_STATE_DIR="$state_dir"
        export PICASU_PORT="${PICASU_PORT:-5673}"
        export PICASU_BASE_URL="http://127.0.0.1:$PICASU_PORT"
        prepare_instance_env
        if curl -s -o /dev/null --max-time 2 "$PICASU_BASE_URL/" 2>/dev/null; then
            echo "fuzz: using the backend already serving $PICASU_BASE_URL" >&2
        elif [ -f "$state_dir/seed.env" ]; then
            build_binaries
            boot_seeded_server "$state_dir"
        else
            seed_instance "$state_dir"
            # seed.sh leaves a bootstrap-mode server up; restart it with the
            # steady-state boot environment before fuzzing.
            boot_seeded_server "$state_dir"
        fi
        # shellcheck disable=SC1091
        if [ -f "$state_dir/seed.env" ]; then source "$state_dir/seed.env"; fi
        export PICASU_BASE_URL="http://127.0.0.1:$PICASU_PORT"
    fi

    export FUZZ_SHARE_HEADERS="${FUZZ_SHARE_HEADERS:-1}"
    export FUZZ_MARKERS="${FUZZ_MARKERS:-1}"
    maybe_login

    local time_args=()
    if [[ " ${args[*]-} " == *" --max-time "* ]]; then
        time_args=("${args[@]}")
    else
        time_args=(--max-time "$((minutes * 60))" "${args[@]}")
    fi

    cd "$REPO_ROOT"
    mkdir -p utils/fuzz/out
    local report="utils/fuzz/out/report.json" st_rc=0
    rm -f "$report"
    echo "campaign: ${minutes}min against $PICASU_BASE_URL"
    set +e
    uvx schemathesis@"$VERSION" --config-file utils/fuzz/campaign.toml --no-color \
        run backend/openapi.json --url "$PICASU_BASE_URL" \
        "${time_args[@]}" \
        --phases "$PHASES,stateful" -n 8 --max-redirects 0 \
        --checks "$CHECKS" --report-json-path "$report"
    st_rc=$?
    set -e
    if [ "$st_rc" -ne 0 ] && [ "$st_rc" -ne 1 ]; then
        die "schemathesis exited with unexpected code $st_rc"
    fi
    echo "=== baseline compare ==="
    python3 utils/fuzz/check_report.py "$report" utils/fuzz/baseline.yaml
}

cmd_diff() {
    cd "$REPO_ROOT"
    python3 utils/fuzz/diff.py "$@"
}

main() {
    if [ $# -eq 0 ] || [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
        usage
        exit 0
    fi
    local verb="$1"
    shift
    case "$verb" in
        check) cmd_check "$@" ;;
        matrix) cmd_matrix "$@" ;;
        roles) cmd_roles "$@" ;;
        run) cmd_run "$@" ;;
        diff) cmd_diff "$@" ;;
        *) die "unknown verb '$verb' (expected check, matrix, roles, run, diff)" ;;
    esac
}

main "$@"
