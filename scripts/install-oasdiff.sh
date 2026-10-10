#!/usr/bin/env bash
# Install a pinned oasdiff release into ~/.local (idempotent, no root).
# Usage: install-oasdiff.sh <version> <sha256>
set -euo pipefail
source "$(dirname "$0")/lib.sh"

version="${1:?usage: install-oasdiff.sh <version> <sha256>}"
expected_sha="${2:?usage: install-oasdiff.sh <version> <sha256>}"

prefix="$HOME/.local/opt/oasdiff-$version"

# Already installed: the pinned prefix runs the pinned version and the PATH
# entry points at it. Re-running is then a no-op.
if reports_version "$prefix/oasdiff" "oasdiff version $version" --version \
    && link_points_at oasdiff "$prefix/oasdiff"; then
    echo "✓ oasdiff $version already installed — $HOME/.local/bin/oasdiff"
    exit 0
fi

mkdir -p "$HOME/.local/opt"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
archive="$workdir/oasdiff.tar.gz"
echo "→ downloading oasdiff-$version"
fetch_verified \
    "https://github.com/oasdiff/oasdiff/releases/download/v$version/oasdiff_${version}_linux_amd64.tar.gz" \
    "$expected_sha" "$archive"

echo "→ unpacking into $prefix"
rm -rf "$prefix"
mkdir -p "$prefix"
tar -xzf "$archive" -C "$prefix"
ensure_link "$prefix/oasdiff" oasdiff
reports_version "$prefix/oasdiff" "oasdiff version $version" --version \
    || { echo "installed oasdiff reports an unexpected version" >&2; exit 1; }
echo "✓ oasdiff $version installed — $HOME/.local/bin/oasdiff"
