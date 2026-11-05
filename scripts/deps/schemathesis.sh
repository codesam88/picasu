#!/usr/bin/env bash
# scripts/deps/schemathesis.sh — ensure/check schemathesis
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/versions.env"

CHECK="${CHECK:-0}"

ensure_schemathesis() {
    local want="$SCHEMATHESIS_VERSION"
    local installed=""
    if command -v uvx >/dev/null 2>&1; then
        installed="$(uvx schemathesis@"$want" --version 2>/dev/null || true)"
    fi
    if [[ "$installed" == "schemathesis, version $want" ]]; then
        echo "✓ schemathesis $want already cached"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ schemathesis: want $want, got ${installed:-<missing>}"
        return 1
    fi
    if ! command -v uvx >/dev/null 2>&1; then
        echo "uvx not found — install uv first" >&2
        return 1
    fi
    echo "→ prefetching schemathesis $want"
    uvx schemathesis@"$want" --version
    echo "✓ schemathesis $want cached"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure_schemathesis
fi
