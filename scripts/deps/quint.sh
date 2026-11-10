#!/usr/bin/env bash
# scripts/deps/quint.sh — ensure/check quint (npm-pinned, registry-verified)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/versions.env"

CHECK="${CHECK:-0}"

ensure_quint() {
    local want="$QUINT_VERSION"
    local installed=""
    if command -v quint >/dev/null 2>&1; then
        installed="$(quint --version 2>/dev/null || true)"
    fi
    if [[ "$installed" == "$want" ]]; then
        echo "✓ quint $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ quint: want $want, got ${installed:-<missing>}"
        return 1
    fi
    if ! command -v npm >/dev/null 2>&1; then
        echo "npm not found — install node first (see scripts/deps/node-check.sh)" >&2
        return 1
    fi
    echo "→ installing @informalsystems/quint@$want"
    npm install --global "@informalsystems/quint@$want"
    if [[ "$(quint --version 2>/dev/null || true)" != "$want" ]]; then
        echo "installed quint reports an unexpected version" >&2
        return 1
    fi
    echo "✓ quint $want installed"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure_quint
fi
