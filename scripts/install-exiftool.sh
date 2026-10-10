#!/usr/bin/env bash
# Install a pinned ExifTool distribution into ~/.local (idempotent, no root).
# Usage: install-exiftool.sh <version> <sha256>
#
# The distribution is pure Perl and unpacks without root. Upstream publishes
# the version and its checksum at https://exiftool.org/checksums-<version>.txt;
# the archive is fetched from the SourceForge mirror that exiftool.org's own
# download links point at.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

version="${1:?usage: install-exiftool.sh <version> <sha256>}"
expected_sha="${2:?usage: install-exiftool.sh <version> <sha256>}"

prefix="$HOME/.local/opt/Image-ExifTool-$version"

# Already installed: the pinned prefix runs the pinned version and the PATH
# entry points at it. Re-running is then a no-op.
if reports_version "$prefix/exiftool" "$version" -ver \
    && link_points_at exiftool "$prefix/exiftool"; then
    echo "✓ exiftool $version already installed — $HOME/.local/bin/exiftool"
    exit 0
fi

mkdir -p "$HOME/.local/opt"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
archive="$workdir/Image-ExifTool-$version.tar.gz"
echo "→ downloading Image-ExifTool-$version"
fetch_verified \
    "https://downloads.sourceforge.net/project/exiftool/Image-ExifTool-$version.tar.gz" \
    "$expected_sha" "$archive"

echo "→ unpacking into $prefix"
rm -rf "$prefix"
tar -xzf "$archive" -C "$HOME/.local/opt"
ensure_link "$prefix/exiftool" exiftool
reports_version "$prefix/exiftool" "$version" -ver \
    || { echo "installed exiftool reports an unexpected version" >&2; exit 1; }
echo "✓ exiftool $version installed — $HOME/.local/bin/exiftool"
