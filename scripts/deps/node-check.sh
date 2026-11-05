#!/usr/bin/env bash
# scripts/deps/node-check.sh — ensure/check node
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/versions.env"

CHECK="${CHECK:-0}"

ensure_node() {
    local installed=""
    if command -v node >/dev/null 2>&1; then
        installed="$(node --version 2>/dev/null)"
    fi
    local ok=0
    if [[ -n "$installed" && "$installed" == v24.* ]]; then
        ok=1
    fi
    echo "node: ${installed:-<missing>} (supported $NODE_SUPPORTED)"
    if [[ "$ok" -eq 0 ]]; then
        return 1
    fi
    return 0
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure_node
fi
