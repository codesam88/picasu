#!/usr/bin/env bash
# Prefetch a pinned zizmor release into the uv cache (idempotent, no root).
# Usage: install-zizmor.sh <version>
#
# zizmor runs via `uvx`, so there is no installed binary to verify:
# "installing" means resolving the pinned version once, making later runs
# offline-ready. Integrity comes from uv's registry verification; there is
# no upstream-published sha256 to check, unlike the tarball-based
# install-*.sh scripts. (Upstream also publishes GitHub release binaries
# without checksum sidecars, and recommends the PyPI distribution because
# it is pre-compiled.)
set -euo pipefail

version="${1:?usage: install-zizmor.sh <version>}"

command -v uvx >/dev/null 2>&1 || {
    echo "uvx not found — install uv (https://docs.astral.sh/uv/) first" >&2
    exit 1
}

if [ "$(uvx zizmor@"$version" --version 2>/dev/null || true)" = "zizmor $version" ]; then
    echo "✓ zizmor $version already cached"
    exit 0
fi

echo "→ prefetching zizmor $version"
uvx zizmor@"$version" --version
echo "✓ zizmor $version cached"
