#!/usr/bin/env bash
# scripts/deps/uv.sh — ensure/check uv
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$UV_VERSION"
    if command -v uv >/dev/null 2>&1 && uv --version 2>/dev/null | grep -q "uv $want"; then
        echo "✓ uv $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ uv: want $want, got $(uv --version 2>/dev/null || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="x86_64-unknown-linux-gnu"; sha="$UV_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-gnu"; sha="$UV_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64) target="x86_64-apple-darwin"; sha="$UV_SHA256_MACOS_X86_64" ;;
        Darwin-arm64) target="aarch64-apple-darwin"; sha="$UV_SHA256_MACOS_AARCH64" ;;
        *) echo "no prebuilt uv for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    local url="${UV_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/uv.tar.gz"
    echo "→ downloading uv $want ($target)"
    curl -fsSL -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" --strip-components=1
    chmod +x "$HOME/.local/bin/uv"
    [[ -x "$HOME/.local/bin/uvx" ]] && chmod +x "$HOME/.local/bin/uvx"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ uv $want installed — $HOME/.local/bin/uv"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
