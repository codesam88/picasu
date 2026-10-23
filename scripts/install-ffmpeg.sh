#!/usr/bin/env bash
# Install a pinned static ffmpeg build into ~/.local (idempotent, no root).
# Usage: install-ffmpeg.sh <version> <sha256>
#
# The static build is the one the release package ships, so the ffmpeg the
# tests run against is the ffmpeg that reaches the admin's machine. The
# tarball is fetched from a versioned URL and checked against the pinned
# digest; the floating "release" URL is never used, because whatever it
# serves today is not what the tests ran against.
#
# The upstream publishes x86_64 builds only. On other architectures the
# system ffmpeg stays in use and the script says so.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

version="${1:?usage: install-ffmpeg.sh <version> <sha256>}"
expected_sha="${2:?usage: install-ffmpeg.sh <version> <sha256>}"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ;;
    *)
        echo "→ no static ffmpeg build for $(uname -s)-$(uname -m) — the system ffmpeg is used"
        exit 0
        ;;
esac

prefix="$HOME/.local/opt/ffmpeg-$version"

# ffmpeg's -version prints a banner, not a bare version: match the version
# inside its first line.
installed_version() {
    [ -x "$1" ] || return 1
    "$1" -version 2>/dev/null | head -1 | grep -q "ffmpeg version $2"
}

# Already installed: the pinned prefix runs the pinned version. The PATH
# entries are (re)linked every run rather than required: a restored cache
# brings the unpacked prefix but never the symlinks, and re-linking is
# what makes a cache hit a no-op instead of a re-download. Re-running is
# then a no-op on a dev machine too.
if installed_version "$prefix/ffmpeg" "$version"; then
    ensure_link "$prefix/ffmpeg" ffmpeg
    ensure_link "$prefix/ffprobe" ffprobe
    echo "✓ ffmpeg $version already installed — $HOME/.local/bin/ffmpeg"
    exit 0
fi

mkdir -p "$HOME/.local/opt"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
archive="$workdir/ffmpeg-$version-amd64-static.tar.xz"
url="https://johnvansickle.com/ffmpeg/releases/ffmpeg-$version-amd64-static.tar.xz"
echo "→ downloading ffmpeg $version"
fetch_verified "$url" "$expected_sha" "$archive"

echo "→ unpacking into $prefix"
rm -rf "$prefix"
mkdir -p "$prefix"
# The archive holds a directory named after the release; the two binaries
# the backend spawns sit inside it.
tar -xJf "$archive" -C "$prefix" --strip-components=1 \
    "ffmpeg-$version-amd64-static/ffmpeg" "ffmpeg-$version-amd64-static/ffprobe"
ensure_link "$prefix/ffmpeg" ffmpeg
ensure_link "$prefix/ffprobe" ffprobe
installed_version "$prefix/ffmpeg" "$version" \
    || { echo "installed ffmpeg reports an unexpected version" >&2; exit 1; }
echo "✓ ffmpeg $version installed — $HOME/.local/bin/ffmpeg"
