#!/usr/bin/env bash
# scripts/deps/ffmpeg.sh — ensure/check ffmpeg
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/versions.env"
CHECK="${CHECK:-0}"

ensure() {
    local want="$FFMPEG_VERSION"
    local target sha
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target="amd64"; sha="$FFMPEG_SHA256_LINUX_X86_64" ;;
        Linux-aarch64|Linux-arm64) target="arm64"; sha="$FFMPEG_SHA256_LINUX_AARCH64" ;;
        *)
            echo "→ no pinned static ffmpeg for $(uname -s)-$(uname -m) — system ffmpeg used"
            return 0
            ;;
    esac
    # Upstream static builds report the tag-derived version (n9.0.2-20261107).
    if command -v ffmpeg >/dev/null 2>&1 && ffmpeg -version 2>/dev/null | head -1 | grep -q "ffmpeg version n$want"; then
        echo "✓ ffmpeg $want already installed"
        return 0
    fi
    if [[ "$CHECK" -eq 1 ]]; then
        echo "✗ ffmpeg: want $want, got $(ffmpeg -version 2>/dev/null | head -1 || echo '<missing>')"
        return 1
    fi
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp:-}"' EXIT
    local url="${FFMPEG_URL_TEMPLATE//\{version\}/$want}"
    url="${url//\{target\}/$target}"
    local file="$tmp/ffmpeg.tar.xz"
    echo "→ downloading ffmpeg $want ($target)"
    curl -fsSL --retry 3 --retry-all-errors -o "$file" "$url"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$sha  $file" | sha256sum -c -
    else
        echo "$sha  $file" | shasum -a 256 -c -
    fi
    mkdir -p "$HOME/.local/bin"
    # The tarball top dir carries the build variant (ffmpeg-nX.Y.Z-linux64-gpl-9.0),
    # so extract the two binaries by path pattern instead of naming the dir.
    tar -xJf "$file" -C "$tmp" --wildcards '*/bin/ffmpeg' '*/bin/ffprobe'
    install -m755 "$tmp"/*/bin/ffmpeg "$tmp"/*/bin/ffprobe "$HOME/.local/bin/"
    rm -rf "$tmp"
    trap - EXIT
    if ! ffmpeg -version 2>/dev/null | head -1 | grep -q "ffmpeg version n$want"; then
        echo "installed ffmpeg reports an unexpected version" >&2
        return 1
    fi
    echo "✓ ffmpeg $want installed — $HOME/.local/bin/ffmpeg"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-ensure}" in
        check|--check) CHECK=1 ;;
    esac
    ensure
fi
