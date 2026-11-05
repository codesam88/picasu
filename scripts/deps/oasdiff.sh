#!/usr/bin/env bash
# scripts/deps/oasdiff.sh — ensure/check oasdiff
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$OASDIFF_VERSION"
    if command -v oasdiff >/dev/null 2>&1 && oasdiff --version 2>/dev/null | grep -q "oasdiff version $want"; then
        echo "✓ oasdiff $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ oasdiff: want $want, got $(oasdiff --version 2>/dev/null || echo '<missing>')"
        return 1
    fi
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="linux_amd64"; sha="$OASDIFF_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="linux_arm64"; sha="$OASDIFF_SHA256_LINUX_AARCH64" ;;
        Darwin-x86_64|Darwin-arm64) target="darwin_all"; sha="$OASDIFF_SHA256_MACOS_UNIVERSAL" ;;
        *) echo "no prebuilt oasdiff for $(uname -s)-$(uname -m)" >&2; return 1 ;;
    esac
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    local url="${OASDIFF_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/oasdiff.tar.gz"
    echo "→ downloading oasdiff-$want ($target)"
    curl -fsSL -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" oasdiff
    chmod +x "$HOME/.local/bin/oasdiff"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ oasdiff $want installed — $HOME/.local/bin/oasdiff"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
