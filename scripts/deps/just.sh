#!/usr/bin/env bash
# scripts/deps/just.sh — ensure/check just
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$JUST_VERSION"
    if command -v just >/dev/null 2>&1 && just --version 2>/dev/null | grep -qx "just $want"; then
        echo "✓ just $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ just: want $want, got $(just --version 2>/dev/null || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="x86_64-unknown-linux-musl"; sha="$JUST_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-musl"; sha="$JUST_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64) target="x86_64-apple-darwin"; sha="$JUST_SHA256_MACOS_X86_64" ;;
        Darwin-arm64) target="aarch64-apple-darwin"; sha="$JUST_SHA256_MACOS_AARCH64" ;;
        *) echo "no prebuilt just for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp:-}"' EXIT
    local url="${JUST_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/just.tar.gz"
    echo "→ downloading just $want ($target)"
    curl -fsSL --retry 3 --retry-all-errors -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" just
    chmod +x "$HOME/.local/bin/just"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ just $want installed — $HOME/.local/bin/just"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
