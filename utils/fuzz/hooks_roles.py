"""Per-role Schemathesis hooks: role-aware auth injection + refresh + poison guards.

Auth wiring:
- admin/member/disabled roles: Cookie header injected from PICASU_JWT.
- snapshot-bearer role: Authorization: Bearer from FUZZ_SNAPSHOT_TOKEN.
- share-guest role: x-album-id / x-share-id on GuardPresenter-guarded ops
  when FUZZ_SHARE_HEADERS=1 (plus optional x-share-password).
- anonymous role: no auth headers at all.

Refresh-on-401: a single re-login (session) or re-prefetch (snapshot) is
attempted when the current credential stops being accepted; counters are
printed at process exit. Disabled credentials are marked exhausted after the
first failed attempt so refresh never loops.

Poison guards mirror hooks_check.py / hooks_campaign.py and additionally
cover index rebuild, read-only toggle, user disable, and signing-key reset.
"""
import atexit
import base64
import json
import os
import sys
import urllib.error
import urllib.request

import schemathesis

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import auth_rules

BASE_URL = os.environ.get("PICASU_BASE_URL", "http://127.0.0.1:8899")
JWT = os.environ.get("PICASU_JWT", "")
SNAPSHOT_TOKEN = os.environ.get("FUZZ_SNAPSHOT_TOKEN", "")
SHARE_HEADERS_ON = os.environ.get("FUZZ_SHARE_HEADERS", "0") == "1"
PASSWORD = os.environ.get("FUZZ_PASSWORD", "FuzzPass!234")

MATRIX_PATH = os.environ.get(
    "FUZZ_AUTH_MATRIX",
    os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "..", "backend", "auth-matrix.json"
    ),
)
MATRIX = auth_rules.load_matrix(MATRIX_PATH)
SHARE_GUARDED = auth_rules.share_guarded_ops(MATRIX)
SNAPSHOT_TYPED_OPS = {
    (k[0], k[1]) for k in (auth_rules.op_key(r) for r in auth_rules.snapshot_typed_ops(MATRIX))
}

# Refresh state.
_REFRESH_ATTEMPTS = 0
_REFRESH_SUCCESSES = 0
_REFRESH_FAILURES = 0
_SESSION_EXHAUSTED = False
_SNAPSHOT_EXHAUSTED = False


def _jwt_payload(token):
    try:
        parts = token.split(".")
        if len(parts) < 2:
            return {}
        pad = "=" * (-len(parts[1]) % 4)
        raw = base64.urlsafe_b64decode(parts[1] + pad)
        return json.loads(raw.decode("utf-8", "replace"))
    except Exception:
        return {}


def _jwt_subject(token):
    payload = _jwt_payload(token)
    role = payload.get("role") or {}
    if isinstance(role, dict):
        if "id" in role:
            return role.get("id")
        user = role.get("user") or {}
        if isinstance(user, dict):
            return user.get("id")
    return None


def _jwt_timestamp(token):
    payload = _jwt_payload(token)
    return payload.get("timestamp")


def _login(user_id):
    global _REFRESH_ATTEMPTS  # noqa: not incremented here
    data = json.dumps({"userId": user_id, "password": PASSWORD}).encode()
    req = urllib.request.Request(
        BASE_URL + "/post/users/authenticate", data=data, method="POST"
    )
    req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            headers = resp.headers
    except urllib.error.HTTPError as exc:
        headers = exc.headers
    except Exception:
        return None
    cookie = headers.get("Set-Cookie", "") if headers else ""
    import re

    match = re.search(r"jwt=([^;]+)", cookie, re.IGNORECASE)
    return match.group(1) if match else None


def _probe_session():
    global JWT, _SESSION_EXHAUSTED, _REFRESH_ATTEMPTS, _REFRESH_SUCCESSES, _REFRESH_FAILURES
    if _SESSION_EXHAUSTED or not JWT:
        return
    valid = True
    try:
        req = urllib.request.Request(
            BASE_URL + "/get/prefetch",
            data=b"{}",
            method="POST",
            headers={"Cookie": "jwt=" + JWT, "Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            if resp.status != 200:
                valid = False
    except urllib.error.HTTPError as exc:
        if exc.code == 401:
            valid = False
    except Exception:
        return
    if valid:
        return
    _REFRESH_ATTEMPTS += 1
    user_id = _jwt_subject(JWT)
    token = _login(user_id) if user_id else None
    if token:
        JWT = token
        _REFRESH_SUCCESSES += 1
    else:
        _REFRESH_FAILURES += 1
        _SESSION_EXHAUSTED = True


def _re_prefetch_snapshot():
    album = os.environ.get("FUZZ_ALBUM_A", "")
    share = os.environ.get("FUZZ_SHARE_A", "")
    headers = {"Content-Type": "application/json"}
    if album:
        headers["x-album-id"] = album
    if share:
        headers["x-share-id"] = share
    pw = os.environ.get("FUZZ_SHARE_PASSWORD_VALUE", "")
    if pw:
        headers["x-share-password"] = pw
    req = urllib.request.Request(
        BASE_URL + "/get/prefetch", data=b"{}", method="POST", headers=headers
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            raw = resp.read()
            data = json.loads(raw.decode())
            if isinstance(data, dict) and "token" in data:
                return data["token"]
    except urllib.error.HTTPError:
        pass
    except Exception:
        pass
    return None


def _probe_snapshot():
    global SNAPSHOT_TOKEN, _SNAPSHOT_EXHAUSTED, _REFRESH_ATTEMPTS, _REFRESH_SUCCESSES, _REFRESH_FAILURES
    if _SNAPSHOT_EXHAUSTED or not SNAPSHOT_TOKEN:
        return
    ts = _jwt_timestamp(SNAPSHOT_TOKEN)
    valid = True
    if ts is not None:
        headers = {"Authorization": "Bearer " + SNAPSHOT_TOKEN}
        album = os.environ.get("FUZZ_ALBUM_A", "")
        share = os.environ.get("FUZZ_SHARE_A", "")
        if album:
            headers["x-album-id"] = album
        if share:
            headers["x-share-id"] = share
        pw = os.environ.get("FUZZ_SHARE_PASSWORD_VALUE", "")
        if pw:
            headers["x-share-password"] = pw
        try:
            req = urllib.request.Request(
                BASE_URL + f"/get/get-data?timestamp={ts}&start=0&end=1",
                headers=headers,
            )
            with urllib.request.urlopen(req, timeout=10) as resp:
                if resp.status != 200:
                    valid = False
        except urllib.error.HTTPError as exc:
            if exc.code == 401:
                valid = False
        except Exception:
            return
    else:
        # No timestamp to probe; assume valid.
        return
    if valid:
        return
    _REFRESH_ATTEMPTS += 1
    new_token = _re_prefetch_snapshot()
    if new_token:
        SNAPSHOT_TOKEN = new_token
        _REFRESH_SUCCESSES += 1
    else:
        _REFRESH_FAILURES += 1
        _SNAPSHOT_EXHAUSTED = True


def _replay(response, new_headers):
    try:
        import requests

        req = response.request
        headers = dict(req.headers or {})
        headers.update(new_headers)
        r = requests.request(
            req.method,
            req.url,
            headers=headers,
            data=req.body,
            timeout=10,
        )
        response.status_code = r.status_code
        response.content = r.content
        response.content_size = len(r.content or b"")
        response.message = getattr(r, "reason_phrase", "") or ""
        try:
            response.headers.clear()
            for k, v in r.headers.items():
                response.headers[k.lower()] = [v]
        except Exception:
            pass
    except Exception:
        pass


def _print_counters():
    print(
        "schemathesis refresh: attempts={} successes={} failures={}".format(
            _REFRESH_ATTEMPTS, _REFRESH_SUCCESSES, _REFRESH_FAILURES
        ),
        file=sys.stderr,
    )


atexit.register(_print_counters)


def _is_session_mode():
    return bool(JWT) and not bool(SNAPSHOT_TOKEN)


def _is_snapshot_mode():
    return bool(SNAPSHOT_TOKEN)


def _is_share_mode():
    return (not JWT) and (not SNAPSHOT_TOKEN) and SHARE_HEADERS_ON


def _strip_auth(headers):
    return {
        k: v
        for k, v in (headers or {}).items()
        if k.lower() not in ("cookie", "authorization", "x-album-id", "x-share-id", "x-share-password")
    }


@schemathesis.hook
def before_call(ctx, case, kwargs):
    global JWT, SNAPSHOT_TOKEN

    headers = _strip_auth(case.headers or {})

    session_mode = _is_session_mode()
    snapshot_mode = _is_snapshot_mode()
    share_mode = _is_share_mode()

    if session_mode:
        if JWT:
            headers["Cookie"] = "jwt=" + JWT
    elif snapshot_mode:
        if SNAPSHOT_TOKEN:
            headers["Authorization"] = "Bearer " + SNAPSHOT_TOKEN
    # share_mode and anonymous: no cookie/bearer

    album = os.environ.get("FUZZ_ALBUM_A", "")
    share = os.environ.get("FUZZ_SHARE_A", "")
    password = os.environ.get("FUZZ_SHARE_PASSWORD_VALUE", "")

    inject_share = SHARE_HEADERS_ON and bool(album) and bool(share) and (
        session_mode or share_mode or snapshot_mode
    )
    # For snapshot role, run-roles disables share headers; respect env.
    if SHARE_HEADERS_ON and not snapshot_mode:
        inject_share = bool(album) and bool(share)
    else:
        if snapshot_mode:
            inject_share = False

    method = (case.method.upper() if case.method else "")
    path = case.path or ""
    if inject_share and (method, path) in SHARE_GUARDED:
        headers["x-album-id"] = album
        headers["x-share-id"] = share
        if password:
            headers["x-share-password"] = password

    case.headers = headers

    op = getattr(ctx, "operation", None)
    if op is None:
        op = getattr(case, "operation", None)
    if op is None:
        return

    method = (getattr(op, "method", "") or "").upper()
    path = getattr(op, "path", "") or ""

    body = case.body
    # Poison guards.
    if method == "PUT" and path == "/put/admin/config" and isinstance(body, dict):
        for key in ("authKey", "readOnlyMode", "port", "address"):
            if key in body:
                body[key] = None
    elif method == "POST" and path == "/post/config/auth-key/reset":
        # Disabled in config; strip auth so it cannot run as a real admin.
        if isinstance(case.headers, dict):
            case.headers.pop("Cookie", None)
            case.headers.pop("Authorization", None)
    elif method == "PUT" and path == "/put/users/admin" and isinstance(body, dict):
        if "userId" in body:
            body["userId"] = "fuzz-nonexistent-user"
    elif method == "PUT" and path == "/put/users/password" and isinstance(body, dict):
        if "userId" in body:
            body["userId"] = "fuzz-nonexistent-user"
    elif method == "PUT" and path == "/put/users/disabled" and isinstance(body, dict):
        if "userId" in body:
            body["userId"] = "fuzz-nonexistent-user"
    elif method == "PUT" and path == "/put/users/sharing" and isinstance(body, dict):
        if "userId" in body:
            body["userId"] = "fuzz-nonexistent-user"
    elif method == "PUT" and path == "/put/delete_share" and isinstance(body, dict):
        body["shareId"] = "fuzz-nonexistent-share"
        body["albumId"] = "fuzz-nonexistent-album"
    elif method == "PUT" and path == "/put/edit_share" and isinstance(body, dict):
        body["albumId"] = "fuzz-nonexistent-album"
        if "shareId" in body:
            body["shareId"] = "fuzz-nonexistent-share"
    elif method == "DELETE" and path == "/delete/delete-data" and isinstance(body, dict):
        body["asset_ids"] = ["fuzz-nonexistent-asset"]
        body["assetIds"] = ["fuzz-nonexistent-asset"]
    elif method == "POST" and path == "/post/index/album" and isinstance(body, dict):
        body["album"] = "fuzz-nonexistent-album"
        if "clear" in body:
            body["clear"] = False
    elif method == "POST" and path == "/post/admin/config/disable-read-only":
        if isinstance(case.headers, dict):
            case.headers.pop("x-picasu-password", None)
            case.headers.pop("X-Picasu-Password", None)
    elif method == "POST" and path == "/post/logout":
        if isinstance(case.headers, dict):
            case.headers.pop("Cookie", None)
            case.headers.pop("Authorization", None)
            case.headers.pop("x-album-id", None)
            case.headers.pop("x-share-id", None)
            case.headers.pop("x-share-password", None)


@schemathesis.hook
def after_call(ctx, case, response):
    global JWT, SNAPSHOT_TOKEN

    # Session role: if a 401 slips through (race / mid-run invalidation),
    # probe and, if refreshed, replay once.
    session_mode = _is_session_mode() or (JWT and not SNAPSHOT_TOKEN)
    if session_mode and getattr(response, "status_code", 0) == 401:
        old = JWT
        _probe_session()
        if JWT and JWT != old:
            _replay(response, {"Cookie": "jwt=" + JWT})

    # Snapshot role: refresh on 401 for snapshot-typed operations only.
    snapshot_mode = _is_snapshot_mode()
    op = getattr(ctx, "operation", None)
    if op is None:
        op = getattr(case, "operation", None)
    method = (getattr(op, "method", "") or "").upper() if op else (case.method or "").upper()
    path = getattr(op, "path", "") or "" if op else (case.path or "")
    if snapshot_mode and getattr(response, "status_code", 0) == 401 and (method, path) in SNAPSHOT_TYPED_OPS:
        old = SNAPSHOT_TOKEN
        _probe_snapshot()
        if SNAPSHOT_TOKEN and SNAPSHOT_TOKEN != old:
            _replay(response, {"Authorization": "Bearer " + SNAPSHOT_TOKEN})
