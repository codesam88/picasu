#!/usr/bin/env bash
# Install or verify the system tools pinned in scripts/deps/versions.env (idempotent, no root).
#
# Usage:
#   scripts/deps-install.sh [--tool <name>]... [--check]
#
# --tool <name>  select one tool (repeatable); default: all tools.
# --check        verify only; do not install or download. Exit 1 on any
#                missing/mismatched tool or out-of-range toolchain.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/deps/versions.env"

CHECK=0
SELECTED=()

usage() {
    cat <<'EOF'
Usage: scripts/deps-install.sh [--tool <name>]... [--check]
Default (no --tool): run every scripts/deps/*.sh (run-parts style).
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tool)
            [[ $# -ge 2 ]] || { echo "--tool requires an argument" >&2; exit 2; }
            SELECTED+=("$(printf '%s' "$2" | tr '_' '-')")
            shift 2
            ;;
        --check) CHECK=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ ${#SELECTED[@]} -eq 0 ]]; then
    for f in "$SCRIPT_DIR/deps/"*.sh; do
        SELECTED+=("$(basename "$f" .sh)")
    done
fi

for t in "${SELECTED[@]}"; do
    case "$t" in
        rust) t=rust-check ;;
        node) t=node-check ;;
    esac
    if [[ ! -x "$SCRIPT_DIR/deps/$t.sh" ]]; then
        echo "unknown tool: $t (no scripts/deps/$t.sh)" >&2
        exit 2
    fi
done

FAIL=0
mark_fail() { FAIL=1; }

for tool in "${SELECTED[@]}"; do
    case "$tool" in
        rust) name=rust-check ;;
        node) name=node-check ;;
        rust-check|node-check) name="$tool" ;;
        *) name="$tool" ;;
    esac
    script="$SCRIPT_DIR/deps/$name.sh"
    if [[ "$CHECK" -eq 1 ]]; then
        CHECK=0 "$script" check || mark_fail
    else
        CHECK=0 "$script" ensure || mark_fail
    fi
done

if [[ "$FAIL" -ne 0 ]]; then
    echo "" >&2
    echo "deps-install: one or more checks failed" >&2
    exit 1
fi

echo ""
echo "deps-install: all selected tools OK"
