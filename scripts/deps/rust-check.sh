#!/usr/bin/env bash
# scripts/deps/rust-check.sh — ensure/check rust
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
# shellcheck disable=SC1091
source "$SCRIPT_DIR/versions.env"

CHECK="${CHECK:-0}"

ensure_rust() {
    local installed=""
    if command -v rustc >/dev/null 2>&1; then
        installed="$(rustc --version 2>/dev/null | awk '{print $2}')"
    fi
    local ok=0
    if [[ -n "$installed" && "$installed" == 1.99* ]]; then
        ok=1
    fi
    echo "rust: ${installed:-<missing>} (supported $RUST_SUPPORTED)"
    if [[ "$ok" -eq 0 ]]; then
        return 1
    fi
    return 0
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure_rust
fi
