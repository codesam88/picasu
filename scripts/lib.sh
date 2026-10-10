#!/usr/bin/env bash
# Shared helpers for scripts/install-*.sh. Source, don't execute:
#   source "$(dirname "$0")/lib.sh"
# Conventions from scripts/README.md apply. All helpers are pure plumbing:
# per-tool version checks and archive layouts stay in the calling script.
set -euo pipefail

# Download $1 to $3 and verify its sha256 is $2. The partial file is removed
# on mismatch. Portable across GNU/BSD checksum tools.
fetch_verified() {
    local url="$1" expected_sha="$2" dest="$3"
    curl -fsSL -o "$dest" "$url"
    local actual
    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$dest" | cut -d' ' -f1)"
    else
        actual="$(shasum -a 256 "$dest" | cut -d' ' -f1)"
    fi
    if [ "$actual" != "$expected_sha" ]; then
        echo "checksum mismatch for $dest" >&2
        echo "  expected $expected_sha" >&2
        echo "  actual   $actual" >&2
        rm -f "$dest"
        return 1
    fi
}

# True iff $1 is executable and prints exactly $2 when run with the rest of
# the arguments. Never fails the caller on a missing binary.
reports_version() {
    local bin="$1" expected="$2"
    shift 2
    [ -x "$bin" ] && [ "$("$bin" "$@" 2>/dev/null || true)" = "$expected" ]
}

# Point ~/.local/bin/$2 at $1, creating the bin dir. Idempotent.
ensure_link() {
    local target="$1" name="$2"
    mkdir -p "$HOME/.local/bin"
    ln -sfn "$target" "$HOME/.local/bin/$name"
}

# True iff ~/.local/bin/$1 already resolves to $2.
link_points_at() {
    local name="$1" target="$2"
    local link="$HOME/.local/bin/$name"
    [ -L "$link" ] && [ "$(readlink -f "$link")" = "$target" ]
}
