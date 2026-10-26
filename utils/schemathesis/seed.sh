#!/usr/bin/env bash
# Bootstrap a disposable Picasu instance for the Schemathesis fuzz harness.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
STATE_DIR="${FUZZ_STATE_DIR:?FUZZ_STATE_DIR must be set}"
DATA_HOME="$STATE_DIR/data"
CONFIG_HOME="$STATE_DIR/config"
IMAGE_HOME="$STATE_DIR/images"
PORT="${PICASU_PORT:-8899}"
BASE="http://127.0.0.1:$PORT"
PW='FuzzPass!234'
ADMIN_USER='admin'
BIN="${PICASU_BINARY:-$REPO_ROOT/target/debug/picasu}"
WEB_ROOT="${PICASU_WEB_ROOT:-$REPO_ROOT/frontend/dist}"
SNAPFAB="${SNAPFAB_BINARY:-}"

mkdir -p "$STATE_DIR"
if [ ! -x "$BIN" ]; then
    echo "backend binary not found: $BIN" >&2
    exit 1
fi

if curl -s -o /dev/null -w '%{http_code}' "$BASE/" 2>/dev/null | grep -q '^200$'; then
    echo "port $PORT already serves a backend; pick another PICASU_PORT" >&2
    exit 1
fi

kill_server() {
    if [ -f "$STATE_DIR/server.pid" ]; then
        pid="$(cat "$STATE_DIR/server.pid")"
        # Scoped to this run's pid — never signal other picasu processes.
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 1 10); do
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.5
        done
        kill -9 "$pid" 2>/dev/null || true
    fi
    sleep 1
}
trap 'kill_server' ERR

kill_server
rm -rf "$DATA_HOME" "$CONFIG_HOME" "$IMAGE_HOME"
mkdir -p "$DATA_HOME" "$CONFIG_HOME"

mkdir -p "$IMAGE_HOME/alpha" "$IMAGE_HOME/beta" "$IMAGE_HOME/gamma"
manifest="$STATE_DIR/manifest.json"
python3 - "$IMAGE_HOME" >"$manifest" <<'PY'
import json, os, sys
base = sys.argv[1]
manifest = []
for album in ("alpha", "beta", "gamma"):
    for i in range(1, 5):
        manifest.append({
            "output": os.path.join(base, album, f"photo_{i:04d}.jpg"),
            "format": "jpeg",
            "minimal": True,
        })
print(json.dumps(manifest))
PY
if [ -n "$SNAPFAB" ] && [ -x "$SNAPFAB" ]; then
    "$SNAPFAB" batch "$manifest" --minimal >/dev/null
else
    cargo run -q -p snapfab -- batch "$manifest" --minimal >/dev/null
fi

start_server() {
    (
        cd "$STATE_DIR"
        env PICASU_PORT="$PORT" \
            PICASU_DATA_HOME="$DATA_HOME" \
            PICASU_CONFIG_HOME="$CONFIG_HOME" \
            PICASU_IMAGE_HOME="$IMAGE_HOME" \
            PICASU_WEB_ROOT="$WEB_ROOT" \
            PICASU_AUTH_KEY='fuzz-auth-key-0123456789abcdef' \
            PICASU_RATE_LIMITS='false' \
            nohup "$BIN" ${BOOTSTRAP_ADMIN:+--bootstrap-admin --trivial-passwords} \
            >"$STATE_DIR/server.log" 2>&1 &
        echo $! >"$STATE_DIR/server.pid"
    )
    for _ in $(seq 1 60); do
        code="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/" || true)"
        [ "$code" = "200" ] && return 0
        sleep 1
    done
    echo "server did not come up" >&2
    cat "$STATE_DIR/server.log" >&2
    return 1
}

login_token() {
    headers="$STATE_DIR/login.headers"
    body="$STATE_DIR/login.body"
    curl -s -D "$headers" -o "$body" -X POST "$BASE/post/users/authenticate" \
        -H 'Content-Type: application/json' \
        -d "{\"userId\":\"$ADMIN_USER\",\"password\":\"$PW\"}" >/dev/null
    sed -n 's/^set-cookie: jwt=\([^;]*\).*/\1/pI' "$headers" | tr -d '\r'
}

BOOTSTRAP_ADMIN=1 start_server

curl -s -X POST "$BASE/post/users/bootstrap" -H 'Content-Type: application/json' \
    -d "{\"userId\":\"$ADMIN_USER\",\"password\":\"$PW\"}" >/dev/null

TOKEN="$(login_token)"
[ -n "$TOKEN" ] || { echo "failed to obtain admin JWT" >&2; exit 1; }
echo "$TOKEN" >"$STATE_DIR/admin.jwt"

curl -s -X POST "$BASE/post/index/album" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"album":"/","clear":true}' >/dev/null
for _ in $(seq 1 120); do
    state="$(curl -s "$BASE/get/index/status" -H "Cookie: jwt=$TOKEN" | python3 -c 'import sys,json; print(json.load(sys.stdin)["state"])')"
    [ "$state" = "completed" ] && break
    [ "$state" = "failed" ] && { echo "index failed" >&2; exit 1; }
    sleep 1
done
state="$(curl -s "$BASE/get/index/status" -H "Cookie: jwt=$TOKEN" | python3 -c 'import sys,json; print(json.load(sys.stdin)["state"])')"
[ "$state" = "completed" ] || { echo "index did not complete (state=$state)" >&2; exit 1; }

kill_server
start_server
TOKEN="$(login_token)"
[ -n "$TOKEN" ] || { echo "failed to obtain admin JWT after restart" >&2; exit 1; }
echo "$TOKEN" >"$STATE_DIR/admin.jwt"

albums="$(curl -s "$BASE/get/get-albums" -H "Cookie: jwt=$TOKEN")"
alpha="$(echo "$albums" | python3 -c 'import sys,json; print(next(a["albumId"] for a in json.load(sys.stdin) if a.get("dirPath")=="alpha"))')"
beta="$(echo "$albums" | python3 -c 'import sys,json; print(next(a["albumId"] for a in json.load(sys.stdin) if a.get("dirPath")=="beta"))')"
gamma="$(echo "$albums" | python3 -c 'import sys,json; print(next(a["albumId"] for a in json.load(sys.stdin) if a.get("dirPath")=="gamma"))')"

share_a="$(curl -s -X POST "$BASE/post/create_share" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"albumId\":\"$alpha\",\"description\":\"fuzz share alpha\",\"showMetadata\":true,\"showDownload\":true,\"showUpload\":false,\"exp\":0}")"
share_b="$(curl -s -X POST "$BASE/post/create_share" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"albumId\":\"$beta\",\"description\":\"fuzz share beta\",\"showMetadata\":true,\"showDownload\":false,\"showUpload\":false,\"exp\":0}")"

curl -s -X POST "$BASE/post/users/register" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"userId":"editor","password":"FuzzPass!234"}' >/dev/null || true

python3 - "$alpha" "$beta" "$gamma" "$share_a" "$share_b" "$STATE_DIR/creds.json" <<'PY'
import json, sys
alpha, beta, gamma, sa, sb, out = sys.argv[1:]
json.dump({
    "alpha_album": alpha,
    "beta_album": beta,
    "gamma_album": gamma,
    "share_alpha": sa,
    "share_beta": sb,
}, open(out, "w"))
PY

rows="$(curl -s -X POST "$BASE/get/prefetch" \
    -H "x-album-id: $alpha" -H "x-share-id: $share_a" \
    -H 'Content-Type: application/json' -d '{}' \
    | python3 -c 'import sys,json; print(json.load(sys.stdin)["prefetch"]["dataLength"])')"
echo "share-alpha snapshot rows: $rows"
[ "$rows" -gt 0 ] || { echo "SEED FAILED: empty share snapshot" >&2; exit 1; }

arows="$(curl -s -X POST "$BASE/get/prefetch" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' -d '{}' \
    | python3 -c 'import sys,json; print(json.load(sys.stdin)["prefetch"]["dataLength"])')"
echo "admin snapshot rows: $arows"

cat >"$STATE_DIR/seed.env" <<EOF
export PICASU_JWT='$TOKEN'
export PICASU_BASE_URL='$BASE'
export FUZZ_ALBUM_A='$alpha'
export FUZZ_SHARE_A='$share_a'
export FUZZ_ALBUM_B='$beta'
export FUZZ_SHARE_B='$share_b'
export FUZZ_PASSWORD='$PW'
EOF

echo "seed ok"
