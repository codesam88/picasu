#!/usr/bin/env bash
# scripts/deps/zizmor.sh — ensure/check zizmor
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/versions.env"

CHECK="${CHECK:-0}"

ensure_zizmor() {
    local want="$ZIZMOR_VERSION"
    local installed=""
    if command -v uvx >/dev/null 2>&1; then
        installed="$(uvx zizmor@"$want" --version 2>/dev/null || true)"
    fi
    if [[ "$installed" == "zizmor $want" ]]; then
        echo "✓ zizmor $want already cached"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ zizmor: want $want, got ${installed:-<missing>}"
        return 1
    fi
    if ! command -v uvx >/dev/null 2>&1; then
        echo "uvx not found — install uv first" >&2
        return 1
    fi
    echo "→ prefetching zizmor $want"
    uvx zizmor@"$want" --version
    echo "✓ zizmor $want cached"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure_zizmor
fi
