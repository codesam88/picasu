#!/usr/bin/env bash
# Run the pinned zizmor release via uvx. Args are passed through to zizmor.
# Version resolves internally from scripts/deps/versions.env.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "$SCRIPT_DIR/deps/versions.env"

export PATH="$HOME/.local/bin:$PATH"
exec uvx --offline zizmor@"$ZIZMOR_VERSION" "$@"
