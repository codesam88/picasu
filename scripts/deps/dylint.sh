#!/usr/bin/env bash
# scripts/deps/dylint.sh — ensure/check dylint
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/versions.env"

CHECK="${CHECK:-0}"

ensure_dylint() {
    local want="$DYLINT_VERSION"
    local installed=""
    if command -v cargo-dylint >/dev/null 2>&1; then
        installed="$(cargo dylint --version 2>/dev/null || true)"
    fi
    if [[ "$installed" == "cargo-dylint $want" ]]; then
        echo "✓ cargo-dylint $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ dylint: want $want, got ${installed:-<missing>}"
        return 1
    fi
    if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo not found — cannot install dylint" >&2
        return 1
    fi
    echo "→ installing cargo-dylint $want"
    cargo install --locked --version "$want" cargo-dylint
    if [[ "$(cargo dylint --version 2>/dev/null || true)" != "cargo-dylint $want" ]]; then
        echo "installed cargo-dylint reports an unexpected version" >&2
        return 1
    fi
    echo "✓ cargo-dylint $want installed"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure_dylint
fi
