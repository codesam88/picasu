#!/usr/bin/env bash
# Install a pinned cargo-nextest binary into ~/.local (idempotent, no root).
# Usage: install-nextest.sh <version> <sha256-linux-x86_64> <sha256-linux-aarch64> <sha256-apple-darwin>
#
# nextest publishes prebuilt binaries per platform on its GitHub release, each
# with a .sha256 sidecar. The source build (`cargo install cargo-nextest`)
# takes minutes, so the binary is what both the dev setup and CI use.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

version="${1:?usage: install-nextest.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-darwin>}"
sha_linux_x86_64="${2:?usage: install-nextest.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-darwin>}"
sha_linux_aarch64="${3:?usage: install-nextest.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-darwin>}"
sha_apple_darwin="${4:?usage: install-nextest.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-darwin>}"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
        target="x86_64-unknown-linux-gnu"
        expected_sha="$sha_linux_x86_64"
        ;;
    Linux-aarch64 | Linux-arm64)
        target="aarch64-unknown-linux-gnu"
        expected_sha="$sha_linux_aarch64"
        ;;
    Darwin-*)
        target="universal-apple-darwin"
        expected_sha="$sha_apple_darwin"
        ;;
    *)
        echo "no prebuilt cargo-nextest for $(uname -s)-$(uname -m); install it with 'cargo install cargo-nextest --locked'" >&2
        exit 1
        ;;
esac

prefix="$HOME/.local/opt/cargo-nextest-$version"

# The version banner carries the build's commit hash, so match the version
# inside its first line instead of the whole output.
installed_version() {
    [ -x "$1" ] || return 1
    "$1" --version 2>/dev/null | head -1 | grep -q "cargo-nextest $2"
}

# Already installed: the pinned prefix runs the pinned version and the PATH
# entry points at it. Re-running is then a no-op.
if installed_version "$prefix/cargo-nextest" "$version" \
    && link_points_at cargo-nextest "$prefix/cargo-nextest"; then
    echo "✓ cargo-nextest $version already installed — $HOME/.local/bin/cargo-nextest"
    exit 0
fi

mkdir -p "$HOME/.local/opt"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
archive="$workdir/cargo-nextest-$version-$target.tar.gz"
url="https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$version/cargo-nextest-$version-$target.tar.gz"
echo "→ downloading cargo-nextest $version ($target)"
fetch_verified "$url" "$expected_sha" "$archive"

echo "→ unpacking into $prefix"
rm -rf "$prefix"
mkdir -p "$prefix"
tar -xzf "$archive" -C "$prefix" cargo-nextest
ensure_link "$prefix/cargo-nextest" cargo-nextest
installed_version "$prefix/cargo-nextest" "$version" \
    || { echo "installed cargo-nextest reports an unexpected version" >&2; exit 1; }
echo "✓ cargo-nextest $version installed — $HOME/.local/bin/cargo-nextest"
