#!/usr/bin/env bash
# scripts/deps/plan.sh — ensure/check plan
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/versions.env"

CHECK="${CHECK:-0}"

ensure_plan() {
    local want="$PLAN_VERSION"
    local installed=""
    if command -v plan >/dev/null 2>&1; then
        installed="$(plan --version 2>/dev/null || true)"
    fi
    if [[ "$installed" == "plan $want" ]]; then
        echo "✓ plan $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ plan: want $want, got ${installed:-<missing>}"
        return 1
    fi
    if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo not found — cannot install plan" >&2
        return 1
    fi
    echo "→ installing plan $want"
    cargo install --locked --git https://github.com/tedsamhain/tablethat --tag "v$want"
    if [[ "$(plan --version 2>/dev/null || true)" != "plan $want" ]]; then
        echo "installed plan reports an unexpected version" >&2
        return 1
    fi
    echo "✓ plan $want installed"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure_plan
fi
