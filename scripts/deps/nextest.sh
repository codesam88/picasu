#!/usr/bin/env bash
# scripts/deps/nextest.sh — ensure/check nextest
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$NEXTEST_VERSION"
    if command -v cargo-nextest >/dev/null 2>&1 && cargo-nextest --version 2>/dev/null | head -1 | grep -q "cargo-nextest $want"; then
        echo "✓ cargo-nextest $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ nextest: want $want, got $(cargo-nextest --version 2>/dev/null | head -1 || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="x86_64-unknown-linux-gnu"; sha="$NEXTEST_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-gnu"; sha="$NEXTEST_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64|Darwin-arm64) target="universal-apple-darwin"; sha="$NEXTEST_SHA256_MACOS_UNIVERSAL" ;;
        *) echo "no prebuilt nextest for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    local url="${NEXTEST_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/nextest.tar.gz"
    echo "→ downloading cargo-nextest $want ($target)"
    curl -fsSL -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" cargo-nextest
    chmod +x "$HOME/.local/bin/cargo-nextest"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ cargo-nextest $want installed — $HOME/.local/bin/cargo-nextest"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
