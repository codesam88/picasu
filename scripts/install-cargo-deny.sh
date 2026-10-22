#!/usr/bin/env bash
# Install a pinned cargo-deny binary into ~/.local (idempotent, no root).
# Usage: install-cargo-deny.sh <version> <sha256-linux-x86_64> <sha256-linux-aarch64> <sha256-apple-darwin-x86_64> <sha256-apple-darwin-aarch64>
#
# cargo-deny publishes prebuilt binaries per platform on its GitHub release,
# each with a .sha256 sidecar. The source build (`cargo install cargo-deny`)
# takes minutes, so the binary is what CI uses.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

version="${1:?usage: install-cargo-deny.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-x86_64> <sha-apple-aarch64>}"
sha_linux_x86_64="${2:?usage: install-cargo-deny.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-x86_64> <sha-apple-aarch64>}"
sha_linux_aarch64="${3:?usage: install-cargo-deny.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-x86_64> <sha-apple-aarch64>}"
sha_apple_x86_64="${4:?usage: install-cargo-deny.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-x86_64> <sha-apple-aarch64>}"
sha_apple_aarch64="${5:?usage: install-cargo-deny.sh <version> <sha-linux-x86_64> <sha-linux-aarch64> <sha-apple-x86_64> <sha-apple-aarch64>}"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
        target="x86_64-unknown-linux-musl"
        expected_sha="$sha_linux_x86_64"
        ;;
    Linux-aarch64 | Linux-arm64)
        target="aarch64-unknown-linux-musl"
        expected_sha="$sha_linux_aarch64"
        ;;
    Darwin-x86_64)
        target="x86_64-apple-darwin"
        expected_sha="$sha_apple_x86_64"
        ;;
    Darwin-arm64)
        target="aarch64-apple-darwin"
        expected_sha="$sha_apple_aarch64"
        ;;
    *)
        echo "no prebuilt cargo-deny for $(uname -s)-$(uname -m); install it with 'cargo install cargo-deny --locked'" >&2
        exit 1
        ;;
esac

prefix="$HOME/.local/opt/cargo-deny-$version"

# Already installed: the pinned prefix runs the pinned version and the PATH
# entry points at it. Re-running is then a no-op.
if reports_version "$prefix/cargo-deny" "cargo-deny $version" --version \
    && link_points_at cargo-deny "$prefix/cargo-deny"; then
    echo "✓ cargo-deny $version already installed — $HOME/.local/bin/cargo-deny"
    exit 0
fi

mkdir -p "$HOME/.local/opt"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
archive="$workdir/cargo-deny-$version-$target.tar.gz"
url="https://github.com/EmbarkStudios/cargo-deny/releases/download/$version/cargo-deny-$version-$target.tar.gz"
echo "→ downloading cargo-deny $version ($target)"
fetch_verified "$url" "$expected_sha" "$archive"

echo "→ unpacking into $prefix"
rm -rf "$prefix"
mkdir -p "$prefix"
# The archive holds a directory named after the release; the binary sits
# inside it.
tar -xzf "$archive" -C "$prefix" --strip-components=1 "cargo-deny-$version-$target/cargo-deny"
ensure_link "$prefix/cargo-deny" cargo-deny
reports_version "$prefix/cargo-deny" "cargo-deny $version" --version \
    || { echo "installed cargo-deny reports an unexpected version" >&2; exit 1; }
echo "✓ cargo-deny $version installed — $HOME/.local/bin/cargo-deny"
