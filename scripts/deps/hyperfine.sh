#!/usr/bin/env bash
# scripts/deps/hyperfine.sh — ensure/check hyperfine
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$HYPERFINE_VERSION"
    if command -v hyperfine >/dev/null 2>&1 && hyperfine --version 2>/dev/null | grep -q "hyperfine $want"; then
        echo "✓ hyperfine $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ hyperfine: want $want, got $(hyperfine --version 2>/dev/null || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="x86_64-unknown-linux-gnu"; sha="$HYPERFINE_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-gnu"; sha="$HYPERFINE_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64) target="x86_64-apple-darwin"; sha="$HYPERFINE_SHA256_MACOS_X86_64" ;;
        Darwin-arm64) target="aarch64-apple-darwin"; sha="$HYPERFINE_SHA256_MACOS_AARCH64" ;;
        *) echo "no prebuilt hyperfine for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp:-}"' EXIT
    local url="${HYPERFINE_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/hyperfine.tar.gz"
    echo "→ downloading hyperfine-$want ($target)"
    curl -fsSL --retry 3 --retry-all-errors -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$tmp"
    install -m755 "$tmp"/*/hyperfine "$HOME/.local/bin/hyperfine"
    rm -rf "$tmp"
    trap - EXIT
    if ! hyperfine --version 2>/dev/null | grep -q "hyperfine $want"; then
        echo "installed hyperfine reports an unexpected version" >&2
        return 1
    fi
    echo "✓ hyperfine $want installed — $HOME/.local/bin/hyperfine"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi