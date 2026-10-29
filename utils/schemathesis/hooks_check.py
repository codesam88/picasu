"""Tier-1 Schemathesis hooks: share-header injection plus run-poisoning guards.

Auth wiring (securitySchemes are declared now, but Schemathesis cannot perform
the login exchange, so the harness still injects the cookie itself):
- admin session: static Cookie header supplied via schemathesis config from
  ${PICASU_JWT}.
- share identity: x-album-id / x-share-id headers on the GuardPresenter-guarded
  operations, so those routes exercise the guest path.
- Run-poisoning guards: prevent the fuzzer from rotating authKey, flipping
  readOnlyMode/port/address, demoting the admin, or deleting seeded
  shares/data mid-run.
"""

import json
import os
import urllib.request

import schemathesis

BASE_URL = os.environ.get("PICASU_BASE_URL", "http://127.0.0.1:8899")
JWT = os.environ.get("PICASU_JWT", "")

_EXPORT = None


def _current_config():
    global _EXPORT
    if _EXPORT is None and JWT:
        try:
            req = urllib.request.Request(
                f"{BASE_URL}/get/config/export", headers={"Cookie": f"jwt={JWT}"}
            )
            with urllib.request.urlopen(req, timeout=10) as resp:
                _EXPORT = json.loads(resp.read().decode())
        except Exception:
            _EXPORT = None
    return _EXPORT

SHARE_GUARDED = {
    ("POST", "/get/prefetch"),
    ("GET", "/get/config"),
    ("PUT", "/put/set_album_title"),
    ("PUT", "/put/set_user_defined_description"),
    ("GET", "/object/compressed/{file_path}"),
    ("GET", "/object/imported/{file_path}"),
}


@schemathesis.hook
def before_call(ctx, case, kwargs):
    if (case.method.upper() if case.method else "", case.path) in SHARE_GUARDED:
        case.headers = {
            **(case.headers or {}),
            "x-album-id": os.environ["FUZZ_ALBUM_A"],
            "x-share-id": os.environ["FUZZ_SHARE_A"],
        }

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
    elif method == "POST" and path == "/post/config/auth-key/reset":
        # Disabled in config; guard is a no-op if it ever runs.
        return
    elif method == "PUT" and path == "/put/users/admin" and isinstance(body, dict):
        if body.get("userId") == "admin" and body.get("admin") is False:
            body["admin"] = True
    elif method == "PUT" and path == "/put/delete_share" and isinstance(body, dict):
        body["shareId"] = "fuzz-nonexistent-share"
        body["albumId"] = "fuzz-nonexistent-album"
    elif method == "PUT" and path == "/put/edit_share" and isinstance(body, dict):
        body["albumId"] = "fuzz-nonexistent-album"
    elif method == "DELETE" and path == "/delete/delete-data" and isinstance(body, dict):
        body["asset_ids"] = ["fuzz-nonexistent-asset"]
        body["assetIds"] = ["fuzz-nonexistent-asset"]
    elif method == "POST" and path == "/post/logout":
        # Never revoke the shared admin session mid-run.
        if isinstance(case.headers, dict):
            case.headers.pop("Cookie", None)
    elif method == "PUT" and path == "/put/users/password":
        # Never rotate the admin password mid-run.
        if isinstance(body, dict) and body.get("userId") == "admin":
            body["userId"] = "fuzz-nonexistent-user"
