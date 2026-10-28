#!/usr/bin/env bash
# Seed (first run) and serve a persistent Picasu instance in the foreground.
# Usage: serve.sh [picasu args...]
# State: $FUZZ_STATE_DIR or sandbox/fuzz (stable across runs; delete the dir
# to reseed). Stop with Ctrl-C.
set -euo pipefail

ORIG_PWD="$(pwd)"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

STATE_DIR_INPUT="${FUZZ_STATE_DIR:-$REPO_ROOT/sandbox/fuzz}"
case "$STATE_DIR_INPUT" in
    /*) STATE_DIR="$STATE_DIR_INPUT" ;;
    *) STATE_DIR="$ORIG_PWD/$STATE_DIR_INPUT" ;;
esac
export FUZZ_STATE_DIR="$STATE_DIR"

cd "$REPO_ROOT"

PORT="${PICASU_PORT:-5673}"
export PICASU_PORT="$PORT"
export PICASU_BINARY="${PICASU_BINARY:-$REPO_ROOT/target/debug/picasu}"
export SNAPFAB_BINARY="${SNAPFAB_BINARY:-$REPO_ROOT/target/debug/snapfab}"
export PICASU_WEB_ROOT="${PICASU_WEB_ROOT:-$REPO_ROOT/frontend/dist}"

# A live backend on the port is somebody else's (a foreground run of this
# very script included) — never hijack or double-bind it.
if curl -s -o /dev/null --max-time 2 "http://127.0.0.1:$PORT/" 2>/dev/null; then
    echo "fuzz-serve: port $PORT already serves a backend; pick another PICASU_PORT" >&2
    exit 1
fi
# A leftover background pid can only come from an interrupted seeding run.
if [ -f "$STATE_DIR/server.pid" ]; then
    kill "$(cat "$STATE_DIR/server.pid")" 2>/dev/null || true
    rm -f "$STATE_DIR/server.pid"
fi

if [ ! -f "$STATE_DIR/seed.env" ]; then
    cargo build --bin picasu
    cargo build -p snapfab
    utils/schemathesis/seed.sh # boots a background instance for the seeding dance
    kill "$(cat "$STATE_DIR/server.pid")" 2>/dev/null || true
    rm -f "$STATE_DIR/server.pid"
    sleep 1
fi
# shellcheck disable=SC1091
source "$STATE_DIR/seed.env" 2>/dev/null || true

# Mirror seed.sh's boot environment exactly: the homes pin the server to the
# seeded state (an unpinned boot falls back to the OS data dir and can bind
# the host's real store), the auth key must match the sessions seed.env
# carries, and the seeded store keeps the enrollment window shut regardless
# of a bootstrap flag in an ambient config.
export PICASU_DATA_HOME="$STATE_DIR/data"
export PICASU_CONFIG_HOME="$STATE_DIR/config"
export PICASU_IMAGE_HOME="$STATE_DIR/images"
export PICASU_AUTH_KEY='fuzz-auth-key-0123456789abcdef'
export PICASU_RATE_LIMITS="${PICASU_RATE_LIMITS:-false}"
export PICASU_BOOTSTRAP_ADMIN=false

echo "serving http://127.0.0.1:$PORT (state: $STATE_DIR) — Ctrl-C to stop"
echo "campaign: just fuzz-run   |   env for other tools: source $STATE_DIR/seed.env"
# exec: this shell becomes picasu, so Ctrl-C reaches the server directly and
# no orphan can outlive the recipe.
exec "$PICASU_BINARY" "$@"
