#!/usr/bin/env bash
# Prefetch a pinned Schemathesis release into the uv cache (idempotent, no root).
# Usage: install-schemathesis.sh <version>
#
# Schemathesis runs via `uvx`, so there is no installed binary to verify:
# "installing" means resolving the pinned version once, making later runs
# offline-ready. Integrity comes from uv's registry verification; there is
# no upstream-published sha256 to check, unlike the tarball-based
# install-*.sh scripts.
set -euo pipefail

version="${1:?usage: install-schemathesis.sh <version>}"

command -v uvx >/dev/null 2>&1 || {
    echo "uvx not found — install uv (https://docs.astral.sh/uv/) first" >&2
    exit 1
}

if [ "$(uvx schemathesis@"$version" --version 2>/dev/null || true)" = "schemathesis, version $version" ]; then
    echo "✓ schemathesis $version already cached"
    exit 0
fi

echo "→ prefetching schemathesis $version"
uvx schemathesis@"$version" --version
echo "✓ schemathesis $version cached"
