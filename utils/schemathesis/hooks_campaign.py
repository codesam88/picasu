"""Tier-2 Schemathesis campaign hooks: poison guards plus A1-A5 probes.

One module serves several runs; modes are env-gated:

- ``FUZZ_SHARE_HEADERS=1``  injects the seeded share headers on the six
  GuardShare-guarded operations.
- ``FUZZ_MARKERS=1``        installs the per-operation probes below; the
  ``*_refused`` checks only assert when their marker header is present.
- ``FUZZ_READONLY=1``       ``readonly_405`` asserts every mutating operation
  answers 405.

Auth: cookie via ``PICASU_JWT`` (securitySchemes are declared now, but
Schemathesis cannot perform the login exchange, so the harness injects the
cookie itself). Credentials and fixtures arrive from ``seed.env``; without
them (a bare run against a dev instance) the hooks inject nothing and the
probes stay off, so the campaign still runs unauthenticated instead of
crashing.
Poison guards prevent config writes, admin demotion, share/data deletion,
logout and password rotation from wrecking a running instance.
"""

import json
import os
import re
import urllib.error
import urllib.request

import schemathesis

BASE = os.environ.get("PICASU_BASE_URL", "http://127.0.0.1:8899")
ADMIN_JWT = os.environ.get("PICASU_JWT", "")
ADMIN_COOKIE = "jwt=" + ADMIN_JWT
PASSWORD = os.environ.get("FUZZ_PASSWORD", "FuzzPass!234")

ALBUM_A = {"album": os.environ.get("FUZZ_ALBUM_A", ""), "share": os.environ.get("FUZZ_SHARE_A", "")}
ALBUM_B = {"album": os.environ.get("FUZZ_ALBUM_B", ""), "share": os.environ.get("FUZZ_SHARE_B", "")}

# Probes call the seeded fixtures and can raise when they are absent, so they
# only arm when the whole seed.env set is present (see seed.sh).
FIXTURES_READY = bool(
    ADMIN_JWT
    and ALBUM_A["album"]
    and ALBUM_A["share"]
    and ALBUM_B["album"]
    and ALBUM_B["share"]
)

SHARE_GUARDED = {
    ("POST", "/get/prefetch"),
    ("GET", "/get/config"),
    ("PUT", "/put/set_album_title"),
    ("PUT", "/put/set_user_defined_description"),
    ("GET", "/object/compressed/{file_path}"),
    ("GET", "/object/imported/{file_path}"),
}

READONLY_EXEMPT = {
    "/post/users/authenticate",
    "/post/admin/config/disable-read-only",
    "/post/renew-hash-token",
    "/post/renew-timestamp-token",
    "/post/index/cancel",
    "/get/prefetch",
}

_cache = {}


def _api(method, path, headers=None, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method)
    for key, value in (headers or {}).items():
        req.add_header(key, value)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            raw, code = resp.read(), resp.status
    except urllib.error.HTTPError as exc:
        raw, code = exc.read(), exc.code
    try:
        return code, json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        return code, raw.decode("utf-8", "replace")


def _login_token(user_id, password):
    data = json.dumps({"userId": user_id, "password": password}).encode()
    req = urllib.request.Request(
        BASE + "/post/users/authenticate", data=data, method="POST"
    )
    req.add_header("Content-Type", "application/json")
    headers = None
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            headers = resp.headers
    except urllib.error.HTTPError as exc:
        headers = exc.headers
    cookie = headers.get("Set-Cookie", "") if headers else ""
    match = re.search(r"jwt=([^;]+)", cookie, re.IGNORECASE)
    return match.group(1) if match else None


def _refresh_admin_token():
    global ADMIN_JWT, ADMIN_COOKIE
    code, _ = _api("POST", "/get/prefetch", headers={"Cookie": ADMIN_COOKIE}, body={})
    if code != 401:
        return
    token = _login_token("admin", PASSWORD)
    if token:
        ADMIN_JWT = token
        ADMIN_COOKIE = "jwt=" + token


def _share_snapshot(which):
    key = ("snapshot", which)
    if key not in _cache:
        album = ALBUM_B if which == "B" else ALBUM_A
        code, resp = _api(
            "POST",
            "/get/prefetch",
            headers={"x-album-id": album["album"], "x-share-id": album["share"]},
            body={},
        )
        if not isinstance(resp, dict) or "token" not in resp:
            raise RuntimeError(f"share snapshot failed: status={code} body={resp!r} album={album}")
        _cache[key] = (resp["token"], resp["prefetch"]["timestamp"])
    return _cache[key]


def _album_asset(which):
    key = ("asset", which)
    if key not in _cache:
        code, resp = _api("POST", "/get/prefetch", headers={"Cookie": ADMIN_COOKIE}, body={})
        if not isinstance(resp, dict) or "token" not in resp:
            raise RuntimeError(f"admin snapshot failed: status={code} body={resp!r}")
        ts, tok = resp["prefetch"]["timestamp"], resp["token"]
        rcode, rows = _api(
            "GET",
            f"/get/get-data?timestamp={ts}&start=0&end=500",
            headers={"Authorization": f"Bearer {tok}"},
        )
        if not isinstance(rows, list):
            raise RuntimeError(f"admin data failed: status={rcode} body={rows!r}")
        target = ALBUM_B["album"] if which == "B" else ALBUM_A["album"]
        _cache[key] = next(
            r["abstractData"]["id"]
            for r in rows
            if r["abstractData"].get("type") != "album"
            and r["abstractData"].get("album") == target
        )
    return _cache[key]


def _demoted_token():
    if "demoted" not in _cache:
        _api(
            "PUT",
            "/put/users/admin",
            headers={"Cookie": ADMIN_COOKIE},
            body={"userId": "editor", "admin": True},
        )
        token = _login_token("editor", PASSWORD)
        _api(
            "PUT",
            "/put/users/admin",
            headers={"Cookie": ADMIN_COOKIE},
            body={"userId": "editor", "admin": False},
        )
        _cache["demoted"] = token
    return _cache["demoted"]


def _install_markers(case, headers):
    key = (case.method, case.path)

    if key == ("GET", "/get/metadata/{asset_id}"):
        token, ts = _share_snapshot("A")
        headers["X-Pilot"] = "cross-scope"
        headers["Authorization"] = f"Bearer {token}"
        case.query = {"timestamp": str(ts)}
        case.path_parameters = {"asset_id": _album_asset("B")}

    elif key == ("GET", "/get/get-data"):
        token, ts = _share_snapshot("A")
        headers["X-Pilot"] = "wrong-typ"
        headers["Authorization"] = f"Bearer {ADMIN_JWT}"
        case.query = {"timestamp": str(ts), "start": "0", "end": "5"}

    elif key == ("GET", "/get/users"):
        headers["X-Pilot"] = "demoted"
        headers["Cookie"] = "jwt=" + _demoted_token()

    elif key == ("POST", "/post/renew-timestamp-token"):
        token, _ = _share_snapshot("A")
        headers["X-Pilot"] = "renew-mismatch"
        headers["x-album-id"] = ALBUM_B["album"]
        headers["x-share-id"] = ALBUM_B["share"]
        case.body = {"token": token}


@schemathesis.hook
def before_call(ctx, case, kwargs):
    headers = dict(case.headers or {})
    if ADMIN_JWT:
        _refresh_admin_token()
        headers["Cookie"] = ADMIN_COOKIE
    if FIXTURES_READY and os.environ.get("FUZZ_SHARE_HEADERS") == "1" and (
        case.method,
        case.path,
    ) in SHARE_GUARDED:
        headers["x-album-id"] = ALBUM_A["album"]
        headers["x-share-id"] = ALBUM_A["share"]
    if FIXTURES_READY and os.environ.get("FUZZ_MARKERS") == "1":
        _install_markers(case, headers)
    case.headers = headers

    op = getattr(ctx, "operation", None)
    if op is None:
        op = getattr(case, "operation", None)
    if op is None:
        return

    method = (getattr(op, "method", "") or "").upper()
    path = getattr(op, "path", "") or ""

    body = case.body
    if method == "PUT" and path == "/put/admin/config" and isinstance(body, dict):
        for key in ("authKey", "readOnlyMode", "port", "address"):
            if key in body:
                body[key] = None
    elif method == "PUT" and path == "/put/users/admin" and isinstance(body, dict):
        # Never touch the admin record mid-run: any write can bump its
        # version and revoke the shared session token.
        # Never mutate real user records through the fuzzer.
        if isinstance(body, dict) and "userId" in body:
            body["userId"] = "fuzz-nonexistent-user"
    elif method == "PUT" and path == "/put/delete_share" and isinstance(body, dict):
        body["shareId"] = "fuzz-nonexistent-share"
        body["albumId"] = "fuzz-nonexistent-album"
    elif method == "PUT" and path == "/put/edit_share" and isinstance(body, dict):
        body["albumId"] = "fuzz-nonexistent-album"
    elif method == "DELETE" and path == "/delete/delete-data" and isinstance(body, dict):
        body["asset_ids"] = ["fuzz-nonexistent-asset"]
        body["assetIds"] = ["fuzz-nonexistent-asset"]
    elif method == "POST" and path == "/post/logout":
        if isinstance(case.headers, dict):
            case.headers.pop("Cookie", None)
    elif method == "PUT" and path == "/put/users/password":
        # Never exercise password changes through the fuzzer: wrong guesses
        # can revoke the shared session via the password throttle.
        # Never mutate real user records through the fuzzer.
        if isinstance(body, dict) and "userId" in body:
            body["userId"] = "fuzz-nonexistent-user"


def _marker(case, name):
    return (case.headers or {}).get("X-Pilot") == name


@schemathesis.check
def cross_scope_refused(ctx, response, case):
    if _marker(case, "cross-scope") and response.status_code not in (401, 403, 404):
        raise AssertionError(
            f"cross-scope read not refused: got {response.status_code} for "
            f"{case.formatted_path}"
        )


@schemathesis.check
def wrong_typ_rejected(ctx, response, case):
    if _marker(case, "wrong-typ") and response.status_code != 401:
        raise AssertionError(
            f"wrong-typ token accepted: got {response.status_code} on {case.path}"
        )


@schemathesis.check
def demoted_token_rejected(ctx, response, case):
    if _marker(case, "demoted") and response.status_code != 401:
        raise AssertionError(
            f"demoted token still accepted: got {response.status_code} on {case.path}"
        )


@schemathesis.check
def cross_share_renewal_refused(ctx, response, case):
    if _marker(case, "renew-mismatch") and response.status_code != 401:
        raise AssertionError(
            f"cross-share renewal not refused: got {response.status_code}"
        )


@schemathesis.check
def readonly_405(ctx, response, case):
    if os.environ.get("FUZZ_READONLY") != "1":
        return
    if case.method == "GET" or case.path in READONLY_EXEMPT:
        return
    if response.status_code != 405:
        raise AssertionError(
            f"mutation not blocked under read-only: {case.method} {case.path} "
            f"-> {response.status_code}"
        )
