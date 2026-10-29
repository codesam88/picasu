"""Tier-2 Schemathesis campaign hooks: poison guards plus A1-A5 probes.

One module serves several runs; modes are env-gated:

- ``FUZZ_SHARE_HEADERS=1``  injects the seeded share headers on the
  GuardPresenter-guarded operations.
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

Which operation carries which probe, the share-header injection set and the
read-only exemptions are derived from ``backend/auth-matrix.json`` by the
pure functions in ``auth_rules.py`` (unit-tested in
``test_auth_rules.py``); the destructive-operation disables stay hand-tuned
in ``campaign.toml``.
"""

import json
import os
import re
import sys
import urllib.error
import urllib.request

import schemathesis

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import auth_rules

MATRIX_PATH = os.environ.get(
    "FUZZ_AUTH_MATRIX",
    os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "..", "backend", "auth-matrix.json"
    ),
)
MATRIX = auth_rules.load_matrix(MATRIX_PATH)
SHARE_GUARDED = auth_rules.share_guarded_ops(MATRIX)
CHECK_PLAN = auth_rules.test_plan(MATRIX)
READONLY_EXEMPT = auth_rules.readonly_exempt_ops(MATRIX)
# A3 targets are snapshot-typed (auth_rules.wrong_credential_targets), so the
# wrong token for them is the session-typed one.
WRONG_CREDENTIAL_FOR_SNAPSHOT = auth_rules.wrong_credential_token(("snapshot",))

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

# S4 scope pairs: every seeded share identity crossed with the album it must
# not reach. Probes take the first mismatch (share A vs album B).
SCOPE_MISMATCHES = auth_rules.scope_mismatch_pairs(
    [(ALBUM_A["album"], ALBUM_A["share"]), (ALBUM_B["album"], ALBUM_B["share"])]
)

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


def _share_snapshot(album_id, share_id):
    """Mint a share-scoped snapshot token for the given scope pair member."""
    key = ("snapshot", album_id)
    if key not in _cache:
        code, resp = _api(
            "POST",
            "/get/prefetch",
            headers={"x-album-id": album_id, "x-share-id": share_id},
            body={},
        )
        if not isinstance(resp, dict) or "token" not in resp:
            raise RuntimeError(
                f"share snapshot failed: status={code} body={resp!r} album={album_id}"
            )
        _cache[key] = (resp["token"], resp["prefetch"]["timestamp"])
    return _cache[key]


def _album_asset(album_id):
    """First indexed asset id of the given album (admin view)."""
    key = ("asset", album_id)
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
        _cache[key] = next(
            r["abstractData"]["id"]
            for r in rows
            if r["abstractData"].get("type") != "album"
            and r["abstractData"].get("album") == album_id
        )
    return _cache[key]


def _token_for_credential(kind):
    """Token sources by credential kind: the admin session or a share-A snapshot."""
    if kind == "session":
        return ADMIN_JWT
    if kind == "snapshot":
        (own_album, share), _ = SCOPE_MISMATCHES[0]
        return _share_snapshot(own_album, share)[0]
    raise RuntimeError(f"no token source for credential kind {kind!r}")


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


def _snapshot_query(case, timestamp):
    """Pin the snapshot timestamp (must equal the token's claim) and bound
    timeline ranges so probes stay cheap; other generated params survive."""
    query = dict(case.query or {})
    query["timestamp"] = str(timestamp)
    if "start" in query:
        query["start"] = "0"
    if "end" in query:
        query["end"] = "5"
    return query


def _install_markers(case, headers):
    """Attach the probe the auth matrix assigns to this operation."""
    probe = CHECK_PLAN.get((case.method.upper(), case.path))
    if probe is None:
        return
    (own_album, share), (foreign_album, foreign_share) = SCOPE_MISMATCHES[0]

    if probe == auth_rules.TEST_CROSS_SCOPE:
        token, ts = _share_snapshot(own_album, share)
        headers["X-Pilot"] = probe
        headers["Authorization"] = f"Bearer {token}"
        case.query = _snapshot_query(case, ts)
        asset = _album_asset(foreign_album)
        params = dict(case.path_parameters or {})
        for name in re.findall(r"{([^{}]+)}", case.path):
            params[name] = asset
        case.path_parameters = params

    elif probe == auth_rules.TEST_WRONG_CREDENTIAL:
        _, ts = _share_snapshot(own_album, share)
        headers["X-Pilot"] = probe
        headers["Authorization"] = f"Bearer {_token_for_credential(WRONG_CREDENTIAL_FOR_SNAPSHOT)}"
        case.query = _snapshot_query(case, ts)

    elif probe == auth_rules.TEST_DEMOTED:
        headers["X-Pilot"] = probe
        headers["Cookie"] = "jwt=" + _demoted_token()

    elif probe == auth_rules.TEST_RENEW_MISMATCH:
        token, _ = _share_snapshot(own_album, share)
        headers["X-Pilot"] = probe
        headers["x-album-id"] = foreign_album
        headers["x-share-id"] = foreign_share
        case.body = {"token": token}


@schemathesis.hook
def before_call(ctx, case, kwargs):
    headers = dict(case.headers or {})
    if ADMIN_JWT:
        _refresh_admin_token()
        headers["Cookie"] = ADMIN_COOKIE
    if FIXTURES_READY and os.environ.get("FUZZ_SHARE_HEADERS") == "1" and (
        case.method.upper(),
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
    if _marker(case, auth_rules.TEST_CROSS_SCOPE) and (
        response.status_code not in auth_rules.CROSS_SCOPE_EXPECTED_STATUSES
    ):
        raise AssertionError(
            f"cross-scope read not refused: expected "
            f"{auth_rules.CROSS_SCOPE_EXPECTED_STATUSES} for {case.formatted_path}, "
            f"got {response.status_code}"
        )


@schemathesis.check
def wrong_credential_rejected(ctx, response, case):
    if _marker(case, auth_rules.TEST_WRONG_CREDENTIAL) and response.status_code != 401:
        raise AssertionError(
            f"wrong-credential token accepted: got {response.status_code} on {case.path}"
        )


@schemathesis.check
def demoted_token_rejected(ctx, response, case):
    if _marker(case, auth_rules.TEST_DEMOTED) and response.status_code != 401:
        raise AssertionError(
            f"demoted token still accepted: got {response.status_code} on {case.path}"
        )


@schemathesis.check
def cross_share_renewal_refused(ctx, response, case):
    if _marker(case, auth_rules.TEST_RENEW_MISMATCH) and response.status_code != 401:
        raise AssertionError(
            f"cross-share renewal not refused: got {response.status_code}"
        )


@schemathesis.check
def readonly_405(ctx, response, case):
    if os.environ.get("FUZZ_READONLY") != "1":
        return
    key = ((case.method or "").upper(), case.path)
    if key[0] == "GET" or key in READONLY_EXEMPT:
        return
    if response.status_code != 405:
        raise AssertionError(
            f"mutation not blocked under read-only: {case.method} {case.path} "
            f"-> {response.status_code}"
        )
