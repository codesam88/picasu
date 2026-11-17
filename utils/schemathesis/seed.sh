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
SHARE_PW='SharePass!234'
ADMIN_USER='admin0'
BIN="${PICASU_BINARY:-$REPO_ROOT/target/debug/picasu}"
WEB_ROOT="${PICASU_WEB_ROOT:-$REPO_ROOT/frontend/dist}"
SNAPFAB="${SNAPFAB_BINARY:-}"
USERS_HOME="$DATA_HOME/users"

mkdir -p "$STATE_DIR"
if [ ! -x "$BIN" ]; then
    echo "backend binary not found: $BIN" >&2
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

if curl -s -o /dev/null -w '%{http_code}' "$BASE/" 2>/dev/null | grep -q '^200$'; then
    echo "port $PORT already serves a backend; pick another PICASU_PORT" >&2
    exit 1
fi

rm -rf "$DATA_HOME" "$CONFIG_HOME" "$IMAGE_HOME"
mkdir -p "$DATA_HOME" "$CONFIG_HOME" "$USERS_HOME"

# Global image corpus (minimal for speed; mixed formats).
mkdir -p "$IMAGE_HOME/alpha/sub" "$IMAGE_HOME/beta" "$IMAGE_HOME/gamma"
manifest="$STATE_DIR/manifest.json"
python3 - "$IMAGE_HOME" >"$manifest" <<'PY'
import json, os, sys
base = sys.argv[1]
manifest = []
# alpha: minimal + tagged + png
for spec in [
    {"output": os.path.join(base, "alpha", "photo_0001.jpg"), "format": "jpeg", "minimal": True},
    {"output": os.path.join(base, "alpha", "photo_0002.jpg"), "format": "jpeg", "minimal": True, "tags": ["alpha"]},
    {"output": os.path.join(base, "alpha", "photo_0003.png"), "format": "png", "minimal": True},
    {"output": os.path.join(base, "alpha", "photo_0004.jpg"), "format": "jpeg", "minimal": True},
]:
    manifest.append(spec)
# alpha/sub (nested)
for spec in [
    {"output": os.path.join(base, "alpha", "sub", "photo_0001.jpg"), "format": "jpeg", "minimal": True},
    {"output": os.path.join(base, "alpha", "sub", "photo_0002.png"), "format": "png", "minimal": True},
]:
    manifest.append(spec)
# beta
for spec in [
    {"output": os.path.join(base, "beta", "photo_0001.jpg"), "format": "jpeg", "minimal": True},
    {"output": os.path.join(base, "beta", "photo_0002.jpg"), "format": "jpeg", "minimal": True, "tags": ["beta"]},
    {"output": os.path.join(base, "beta", "photo_0003.png"), "format": "png", "minimal": True},
]:
    manifest.append(spec)
# gamma
for spec in [
    {"output": os.path.join(base, "gamma", "photo_0001.jpg"), "format": "jpeg", "minimal": True},
    {"output": os.path.join(base, "gamma", "photo_0002.jpg"), "format": "jpeg", "minimal": True, "tags": ["gamma"]},
    {"output": os.path.join(base, "gamma", "photo_0003.png"), "format": "png", "minimal": True},
]:
    manifest.append(spec)
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
            PICASU_USERS_HOME="$USERS_HOME" \
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
    local user="$1"
    local headers="$STATE_DIR/login.headers.$user"
    local body="$STATE_DIR/login.body.$user"
    curl -s -D "$headers" -o "$body" -X POST "$BASE/post/users/authenticate" \
        -H 'Content-Type: application/json' \
        -d "{\"userId\":\"$user\",\"password\":\"$PW\"}" >/dev/null
    sed -n 's/^set-cookie: jwt=\([^;]*\).*/\1/pI' "$headers" | tr -d '\r'
}

BOOTSTRAP_ADMIN=1 start_server

curl -s -X POST "$BASE/post/users/bootstrap" -H 'Content-Type: application/json' \
    -d "{\"userId\":\"$ADMIN_USER\",\"password\":\"$PW\"}" >/dev/null

TOKEN="$(login_token "$ADMIN_USER")"
[ -n "$TOKEN" ] || { echo "failed to obtain admin JWT" >&2; exit 1; }
echo "$TOKEN" >"$STATE_DIR/admin.jwt"

# Register remaining users via admin session.
register_user() {
    local user="$1"
    curl -s -X POST "$BASE/post/users/register" -H "Cookie: jwt=$TOKEN" \
        -H 'Content-Type: application/json' \
        -d "{\"userId\":\"$user\",\"password\":\"$PW\"}" >/dev/null
}
register_user admin1
register_user memberA
register_user memberB
register_user sharingDisabled
register_user disabledUser

# Elevate admin1 to admin.
curl -s -X PUT "$BASE/put/users/admin" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' -H "X-Picasu-Password: $PW" \
    -d '{"userId":"admin1","admin":true}' >/dev/null

# Login per role.
TOKEN_ADMIN1="$(login_token admin1)"
TOKEN_MEMBER_A="$(login_token memberA)"
TOKEN_MEMBER_B="$(login_token memberB)"
TOKEN_SHARING_DISABLED="$(login_token sharingDisabled)"
TOKEN_DISABLED_DEAD="$(login_token disabledUser)"
[ -n "$TOKEN_ADMIN1" ] || { echo "failed admin1 login" >&2; exit 1; }
[ -n "$TOKEN_MEMBER_A" ] || { echo "failed memberA login" >&2; exit 1; }
[ -n "$TOKEN_MEMBER_B" ] || { echo "failed memberB login" >&2; exit 1; }
[ -n "$TOKEN_SHARING_DISABLED" ] || { echo "failed sharingDisabled login" >&2; exit 1; }
[ -n "$TOKEN_DISABLED_DEAD" ] || { echo "failed disabledUser login" >&2; exit 1; }

# Disable sharingDisabled sharing AFTER creating its dead share later; capture now.
# Create personal-home images for admin0/memberA/memberB.
for u in admin0 memberA memberB; do
    mkdir -p "$USERS_HOME/$u"
done
personal_manifest="$STATE_DIR/personal_manifest.json"
python3 - "$USERS_HOME" >"$personal_manifest" <<'PY'
import json, os, sys
base = sys.argv[1]
manifest = []
for u in ("admin0", "memberA", "memberB"):
    manifest.append({
        "output": os.path.join(base, u, "photo_0001.jpg"),
        "format": "jpeg",
        "minimal": True,
    })
print(json.dumps(manifest))
PY
if [ -n "$SNAPFAB" ] && [ -x "$SNAPFAB" ]; then
    "$SNAPFAB" batch "$personal_manifest" --minimal >/dev/null
else
    cargo run -q -p snapfab -- batch "$personal_manifest" --minimal >/dev/null
fi

# Disable disabledUser account (after login captured).
curl -s -X PUT "$BASE/put/users/disabled" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"userId":"disabledUser","disabled":true}' >/dev/null

# Index global + personal roots.
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

# Resolve album ids.
albums_admin="$STATE_DIR/albums_admin.json"
curl -s "$BASE/get/get-albums" -H "Cookie: jwt=$TOKEN" >"$albums_admin"
albums_ma="$STATE_DIR/albums_memberA.json"
curl -s "$BASE/get/get-albums" -H "Cookie: jwt=$TOKEN_MEMBER_A" >"$albums_ma"
albums_mb="$STATE_DIR/albums_memberB.json"
curl -s "$BASE/get/get-albums" -H "Cookie: jwt=$TOKEN_MEMBER_B" >"$albums_mb"
albums_sd="$STATE_DIR/albums_sharingDisabled.json"
curl -s "$BASE/get/get-albums" -H "Cookie: jwt=$TOKEN_SHARING_DISABLED" >"$albums_sd"

eval "$(python3 - "$albums_admin" "$albums_ma" "$albums_mb" "$albums_sd" <<'PY'
import json, sys
A=json.load(open(sys.argv[1])); MA=json.load(open(sys.argv[2])); MB=json.load(open(sys.argv[3])); SD=json.load(open(sys.argv[4]))
def find(albums, pred, label):
    for a in albums:
        if pred(a): return a["albumId"]
    raise SystemExit("album not found: " + label)
alpha = find(A, lambda a: a.get("dirPath") == "alpha" and a.get("root") == "global", "alpha")
beta = find(A, lambda a: a.get("dirPath") == "beta" and a.get("root") == "global", "beta")
gamma = find(A, lambda a: a.get("dirPath") == "gamma" and a.get("root") == "global", "gamma")
alpha_sub = find(A, lambda a: a.get("dirPath") in ("alpha/sub", "alpha\\sub") and a.get("root") == "global", "alpha/sub")
admin0_home = find(A, lambda a: a.get("root") == "personal" and a.get("owner") == "admin0", "admin0 home")
member_a_home = find(MA, lambda a: a.get("root") == "personal" and a.get("owner") == "memberA", "memberA home")
member_b_home = find(MB, lambda a: a.get("root") == "personal" and a.get("owner") == "memberB", "memberB home")
sharing_disabled_home = find(SD, lambda a: a.get("root") == "personal" and a.get("owner") == "sharingDisabled", "sharingDisabled home")
for name, val in [("alpha",alpha),("beta",beta),("gamma",gamma),("alpha_sub",alpha_sub),("admin0_home",admin0_home),("member_a_home",member_a_home),("member_b_home",member_b_home),("sharing_disabled_home",sharing_disabled_home)]:
    print(name + "='" + val + "'")
PY
)"

# Create shares.
create_share() {
    # args: jwt album_id description showMetadata showDownload showUpload exp [password]
    local jwt="$1" album="$2" desc="$3" sm="$4" sd="$5" su="$6" exp="$7" pw="${8:-}"
    local body
    if [ -n "$pw" ]; then
        body="{\"albumId\":\"$album\",\"description\":\"$desc\",\"password\":\"$pw\",\"showMetadata\":$sm,\"showDownload\":$sd,\"showUpload\":$su,\"exp\":$exp}"
    else
        body="{\"albumId\":\"$album\",\"description\":\"$desc\",\"showMetadata\":$sm,\"showDownload\":$sd,\"showUpload\":$su,\"exp\":$exp}"
    fi
    curl -s -X POST "$BASE/post/create_share" -H "Cookie: jwt=$jwt" \
        -H 'Content-Type: application/json' -d "$body" \
        | python3 -c 'import sys,json; print(json.load(sys.stdin)["link"])'
}

share_open="$(create_share "$TOKEN" "$alpha" "fuzz share open" true true false 0)"
share_nodownload="$(create_share "$TOKEN" "$beta" "fuzz share nodownload" true false false 0)"
share_upload="$(create_share "$TOKEN" "$gamma" "fuzz share upload" true true true 0)"
share_password="$(create_share "$TOKEN" "$alpha_sub" "fuzz share password" true true false 0 "$SHARE_PW")"
share_expired="$(create_share "$TOKEN" "$beta" "fuzz share expired" true true false 1)"
share_member="$(create_share "$TOKEN_MEMBER_A" "$member_a_home" "fuzz share member" true true false 0)"
share_nested="$(create_share "$TOKEN" "$alpha_sub" "fuzz share nested" true true false 0)"
# dead share: create while sharing enabled, then disable sharing.
share_dead="$(create_share "$TOKEN_SHARING_DISABLED" "$sharing_disabled_home" "fuzz share dead" true true false 0)"
curl -s -X PUT "$BASE/put/users/sharing" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"userId":"sharingDisabled","enabled":false}' >/dev/null

# Trash + rate items using admin snapshot timestamp.
prefetch_json="$STATE_DIR/prefetch.json"
curl -s -X POST "$BASE/get/prefetch" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' -d '{}' >"$prefetch_json"
timestamp="$(python3 -c 'import json; print(json.load(open("'"$prefetch_json"'"))["prefetch"]["timestamp"])')"
curl -s -X PUT "$BASE/put/edit_flags" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"indexArray\":[0],\"timestamp\":$timestamp,\"isTrashed\":true}" >/dev/null
curl -s -X PUT "$BASE/put/edit_rating" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"indexArray\":[1],\"timestamp\":$timestamp,\"rating\":5}" >/dev/null

# Invites.
invite_valid_resp="$STATE_DIR/invite_valid.json"
curl -s -X POST "$BASE/post/invites" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"expiresInHours":168}' >"$invite_valid_resp"
invite_valid_token="$(python3 -c 'import json; print(json.load(open("'"$invite_valid_resp"'"))["token"])')"

invite_bound_resp="$STATE_DIR/invite_bound.json"
curl -s -X POST "$BASE/post/invites" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"expiresInHours":168,"username":"memberB"}' >"$invite_bound_resp"
invite_bound_token="$(python3 -c 'import json; print(json.load(open("'"$invite_bound_resp"'"))["token"])')"

invite_used_resp="$STATE_DIR/invite_used.json"
curl -s -X POST "$BASE/post/invites" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"expiresInHours":168}' >"$invite_used_resp"
invite_used_token="$(python3 -c 'import json; print(json.load(open("'"$invite_used_resp"'"))["token"])')"
# Redeem the used invite.
curl -s -X POST "$BASE/post/users/register" \
    -H 'Content-Type: application/json' \
    -d "{\"userId\":\"invitedUser\",\"password\":\"$PW\",\"invite\":\"$invite_used_token\"}" >/dev/null

# Fail-closed verification.
fail() { echo "SEED FAILED: $*" >&2; exit 1; }

[ -n "$TOKEN" ] && [ -n "$TOKEN_ADMIN1" ] && [ -n "$TOKEN_MEMBER_A" ] && [ -n "$TOKEN_MEMBER_B" ] && [ -n "$TOKEN_SHARING_DISABLED" ] || fail "missing role JWT"
[ -n "$alpha" ] && [ -n "$beta" ] && [ -n "$gamma" ] && [ -n "$alpha_sub" ] || fail "missing album id"
[ -n "$admin0_home" ] && [ -n "$member_a_home" ] && [ -n "$member_b_home" ] || fail "missing personal home"

# admin snapshot rows
admin_rows="$(curl -s -X POST "$BASE/get/prefetch" -H "Cookie: jwt=$TOKEN" \
    -H 'Content-Type: application/json' -d '{}' | python3 -c 'import sys,json; print(json.load(sys.stdin)["prefetch"]["dataLength"])')"
[ "$admin_rows" -gt 0 ] || fail "empty admin snapshot"

# member snapshots
for jwt in "$TOKEN_MEMBER_A" "$TOKEN_MEMBER_B"; do
    r="$(curl -s -X POST "$BASE/get/prefetch" -H "Cookie: jwt=$jwt" \
        -H 'Content-Type: application/json' -d '{}' | python3 -c 'import sys,json; print(json.load(sys.stdin)["prefetch"]["dataLength"])')"
    [ "$r" -gt 0 ] || fail "empty member snapshot"
done

# live share resolution checks
verify_live_share() {
    local album="$1" share="$2" pw="${3:-}"
    local code rows
    if [ -n "$pw" ]; then
        code="$(curl -s -o "$STATE_DIR/vs.body" -w '%{http_code}' -X POST "$BASE/get/prefetch" \
            -H "x-album-id: $album" -H "x-share-id: $share" -H "x-share-password: $pw" \
            -H 'Content-Type: application/json' -d '{}')"
    else
        code="$(curl -s -o "$STATE_DIR/vs.body" -w '%{http_code}' -X POST "$BASE/get/prefetch" \
            -H "x-album-id: $album" -H "x-share-id: $share" \
            -H 'Content-Type: application/json' -d '{}')"
    fi
    [ "$code" = "200" ] || fail "share did not resolve: $share (code=$code)"
    rows="$(python3 -c 'import json; print(json.load(open("'"$STATE_DIR/vs.body"'"))["prefetch"]["dataLength"])')"
    [ "$rows" -gt 0 ] || fail "empty share snapshot: $share"
}
verify_live_share "$alpha" "$share_open"
verify_live_share "$beta" "$share_nodownload"
verify_live_share "$gamma" "$share_upload"
verify_live_share "$alpha_sub" "$share_password" "$SHARE_PW"
verify_live_share "$member_a_home" "$share_member"
verify_live_share "$alpha_sub" "$share_nested"

# Write creds.json.
python3 - "$STATE_DIR/creds.json" <<PY
import json, sys
out = sys.argv[1]
data = {
  "admin0_jwt": "$TOKEN",
  "admin1_jwt": "$TOKEN_ADMIN1",
  "member_a_jwt": "$TOKEN_MEMBER_A",
  "member_b_jwt": "$TOKEN_MEMBER_B",
  "sharing_disabled_jwt": "$TOKEN_SHARING_DISABLED",
  "disabled_user_id": "disabledUser",
  "disabled_user_jwt": "$TOKEN_DISABLED_DEAD",
  "alpha_album": "$alpha",
  "beta_album": "$beta",
  "gamma_album": "$gamma",
  "alpha_sub_album": "$alpha_sub",
  "admin0_home_album": "$admin0_home",
  "member_a_home_album": "$member_a_home",
  "member_b_home_album": "$member_b_home",
  "sharing_disabled_home_album": "$sharing_disabled_home",
  "share_open": "$share_open",
  "share_nodownload": "$share_nodownload",
  "share_upload": "$share_upload",
  "share_password": "$share_password",
  "share_expired": "$share_expired",
  "share_member": "$share_member",
  "share_dead": "$share_dead",
  "share_nested": "$share_nested",
  "share_password_value": "$SHARE_PW",
  "invite_valid": "$invite_valid_token",
  "invite_bound": "$invite_bound_token",
  "invite_used": "$invite_used_token",
  "user_password": "$PW",
}
json.dump(data, open(out, "w"), indent=2)
PY

cat >"$STATE_DIR/seed.env" <<EOF
export PICASU_JWT='$TOKEN'
export PICASU_BASE_URL='$BASE'
export PICASU_JWT_ADMIN1='$TOKEN_ADMIN1'
export PICASU_JWT_MEMBER_A='$TOKEN_MEMBER_A'
export PICASU_JWT_MEMBER_B='$TOKEN_MEMBER_B'
export PICASU_JWT_SHARING_DISABLED='$TOKEN_SHARING_DISABLED'
export PICASU_JWT_DISABLED='$TOKEN_DISABLED_DEAD'
export DISABLED_USER_ID='disabledUser'
export FUZZ_ALBUM_A='$alpha'
export FUZZ_ALBUM_B='$beta'
export FUZZ_ALBUM_ALPHA='$alpha'
export FUZZ_ALBUM_BETA='$beta'
export FUZZ_ALBUM_GAMMA='$gamma'
export FUZZ_ALBUM_ALPHA_SUB='$alpha_sub'
export FUZZ_ALBUM_ADMIN0_HOME='$admin0_home'
export FUZZ_ALBUM_MEMBER_A_HOME='$member_a_home'
export FUZZ_ALBUM_MEMBER_B_HOME='$member_b_home'
export FUZZ_SHARE_A='$share_open'
export FUZZ_SHARE_B='$share_nodownload'
export FUZZ_SHARE_OPEN='$share_open'
export FUZZ_SHARE_NODOWNLOAD='$share_nodownload'
export FUZZ_SHARE_UPLOAD='$share_upload'
export FUZZ_SHARE_PASSWORD='$share_password'
export FUZZ_SHARE_EXPIRED='$share_expired'
export FUZZ_SHARE_MEMBER='$share_member'
export FUZZ_SHARE_DEAD='$share_dead'
export FUZZ_SHARE_NESTED='$share_nested'
export FUZZ_PASSWORD='$PW'
export FUZZ_SHARE_PASSWORD_VALUE='$SHARE_PW'
export FUZZ_INVITE_VALID='$invite_valid_token'
export FUZZ_INVITE_BOUND='$invite_bound_token'
export FUZZ_INVITE_USED='$invite_used_token'
EOF

echo "seed ok"