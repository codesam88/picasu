#!/usr/bin/env bash
# scripts/deps/ffmpeg.sh — ensure/check ffmpeg
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$FFMPEG_VERSION"
    if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
        echo "→ no static ffmpeg build for $(uname -s)-$(uname -m) — system ffmpeg used"
        return 0
    fi
    if command -v ffmpeg >/dev/null 2>&1 && ffmpeg -version 2>/dev/null | head -1 | grep -q "ffmpeg version $want"; then
        echo "✓ ffmpeg $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ ffmpeg: want $want, got $(ffmpeg -version 2>/dev/null | head -1 || echo '<missing>')"
        return 1
    fi
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    local url="${FFMPEG_URL_TEMPLATE//\{version\}/$want}"
    local file="$tmp/ffmpeg.tar.xz"
    echo "→ downloading ffmpeg $want"
    curl -fsSL -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$FFMPEG_SHA256  $file" | sha256sum -c -
    else
        echo "$FFMPEG_SHA256  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    tar -xJf "$file" -C "$HOME/.local/bin" --strip-components=1 "ffmpeg-$want-amd64-static/ffmpeg" "ffmpeg-$want-amd64-static/ffprobe"
    chmod +x "$HOME/.local/bin/ffmpeg" "$HOME/.local/bin/ffprobe"
    rm -rf "$tmp"
    trap - EXIT
    echo "✓ ffmpeg $want installed — $HOME/.local/bin/ffmpeg"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
