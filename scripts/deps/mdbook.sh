#!/usr/bin/env bash
# scripts/deps/mdbook.sh — ensure/check mdbook
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$MDBOOK_VERSION"
    if command -v mdbook >/dev/null 2>&1 && mdbook --version 2>/dev/null | grep -q "mdbook v$want"; then
        echo "✓ mdbook $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ mdbook: want $want, got $(mdbook --version 2>/dev/null || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="x86_64-unknown-linux-gnu"; sha="$MDBOOK_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-musl"; sha="$MDBOOK_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64) target="x86_64-apple-darwin"; sha="$MDBOOK_SHA256_MACOS_X86_64" ;;
        Darwin-arm64) target="aarch64-apple-darwin"; sha="$MDBOOK_SHA256_MACOS_AARCH64" ;;
        *) echo "no prebuilt mdbook for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    local url="${MDBOOK_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/mdbook.tar.gz"
    echo "→ downloading mdbook $want ($target)"
    curl -fsSL -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" mdbook
    chmod +x "$HOME/.local/bin/mdbook"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ mdbook $want installed — $HOME/.local/bin/mdbook"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
