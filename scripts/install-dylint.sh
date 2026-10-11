#!/usr/bin/env bash
# Install a pinned cargo-dylint release (idempotent, no root).
# Usage: install-dylint.sh <version>
#
# Integrity comes from crates.io's registry verification plus
# `cargo install --locked` (the lockfile pins every dependency);
# there is no upstream-published sha256 to check, unlike the
# tarball-based install-*.sh scripts.
set -euo pipefail

version="${1:?usage: install-dylint.sh <version>}"

if [ "$(cargo dylint --version 2>/dev/null || true)" = "cargo-dylint $version" ]; then
    echo "✓ cargo-dylint $version already installed"
    exit 0
fi

echo "→ installing cargo-dylint $version"
cargo install --locked --version "$version" cargo-dylint

[ "$(cargo dylint --version)" = "cargo-dylint $version" ] \
    || { echo "installed cargo-dylint reports an unexpected version" >&2; exit 1; }
echo "✓ cargo-dylint $version installed"
