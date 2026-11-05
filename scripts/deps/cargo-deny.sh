#!/usr/bin/env bash
# scripts/deps/cargo-deny.sh — ensure/check cargo-deny
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$CARGO_DENY_VERSION"
    if command -v cargo-deny >/dev/null 2>&1 && cargo-deny --version 2>/dev/null | head -1 | grep -q "cargo-deny $want"; then
        echo "✓ cargo-deny $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ cargo-deny: want $want, got $(cargo-deny --version 2>/dev/null | head -1 || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="x86_64-unknown-linux-musl"; sha="$CARGO_DENY_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-musl"; sha="$CARGO_DENY_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64) target="x86_64-apple-darwin"; sha="$CARGO_DENY_SHA256_MACOS_X86_64" ;;
        Darwin-arm64) target="aarch64-apple-darwin"; sha="$CARGO_DENY_SHA256_MACOS_AARCH64" ;;
        *) echo "no prebuilt cargo-deny for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    local url="${CARGO_DENY_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/cargo-deny.tar.gz"
    echo "→ downloading cargo-deny $want ($target)"
    curl -fsSL -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" --strip-components=1 "cargo-deny-$want-$target/cargo-deny"
    chmod +x "$HOME/.local/bin/cargo-deny"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ cargo-deny $want installed — $HOME/.local/bin/cargo-deny"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
