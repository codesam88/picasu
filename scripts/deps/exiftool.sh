#!/usr/bin/env bash
# scripts/deps/exiftool.sh — ensure/check exiftool
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$EXIFTOOL_VERSION"
    if command -v exiftool >/dev/null 2>&1 && exiftool -ver 2>/dev/null | grep -qx "$want"; then
        echo "✓ exiftool $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ exiftool: want $want, got $(exiftool -ver 2>/dev/null || echo '<missing>')"
        return 1
    fi
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp:-}"' EXIT
    local url="${EXIFTOOL_URL_TEMPLATE//\{version\}/$want}"
    local file="$tmp/exiftool.tar.gz"
    echo "→ downloading Image-ExifTool-$want"
    curl -fsSL --retry 3 --retry-all-errors -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$EXIFTOOL_SHA256  $file" | sha256sum -c -
    else
        echo "$EXIFTOOL_SHA256  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xzf "$file" -C "$HOME/.local/bin" --strip-components=1 "Image-ExifTool-$want/exiftool" "Image-ExifTool-$want/lib"
    chmod +x "$HOME/.local/bin/exiftool"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ exiftool $want installed — $HOME/.local/bin/exiftool"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
