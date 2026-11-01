"""Execute the fuzz matrix: one matrix-derived check per operation.

A campaign driven by the postulated auth-model invariants rather than by
generated input. Every operation the auth matrix assigns a check to is called
once with the exact request that check defines, and the status it must see is
asserted: cross-scope binding, wrong token type, renewal binding, and a
demoted identity.

The check list is rebuilt from ``backend/auth-matrix.json`` on every run, so
guards and token types reach the checks through the contract instead of a hand
list, and no committed copy can go stale. The generative campaign
(``campaign.sh``) keeps the random and sequence-based role.

Requires a seeded instance (`seed.sh`, or `just fuzz-serve`), exporting
``PICASU_BASE_URL``, ``PICASU_JWT``, ``FUZZ_ALBUM_A``, ``FUZZ_SHARE_A``,
``FUZZ_ALBUM_B``, ``FUZZ_SHARE_B`` and ``FUZZ_PASSWORD``.

Usage: python3 utils/schemathesis/fuzz_matrix.py [repo-root]
"""

import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import auth_rules  # noqa: E402  (path set above)

BASE = os.environ.get("PICASU_BASE_URL", "http://127.0.0.1:8899").rstrip("/")
ADMIN_JWT = os.environ.get("PICASU_JWT", "")
ALBUM_A = os.environ.get("FUZZ_ALBUM_A", "")
SHARE_A = os.environ.get("FUZZ_SHARE_A", "")
ALBUM_B = os.environ.get("FUZZ_ALBUM_B", "")
SHARE_B = os.environ.get("FUZZ_SHARE_B", "")
PASSWORD = os.environ.get("FUZZ_PASSWORD", "")
EDITOR = "matrix-editor"


def api(method, path, headers=None, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(BASE + path, data=data, method=method)
    request.add_header("Content-Type", "application/json")
    for name, value in (headers or {}).items():
        request.add_header(name, value)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
            return response.status, decode(raw)
    except urllib.error.HTTPError as error:
        return error.code, decode(error.read())


def decode(raw):
    try:
        return json.loads(raw)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return raw.decode(errors="replace")


def resolve(spec, schema, depth=0):
    if depth > 6 or not isinstance(schema, dict):
        return None
    if "$ref" in schema:
        name = schema["$ref"].split("/")[-1]
        return resolve(spec, spec["components"]["schemas"][name], depth + 1)
    allowed = schema.get("enum")
    if allowed:
        return allowed[0]
    kind = schema.get("type")
    if kind == "string":
        return "x"
    if kind in ("integer", "number"):
        return 1
    if kind == "boolean":
        return False
    if kind == "array":
        return []
    value = {}
    for name in schema.get("required", []):
        value[name] = resolve(spec, schema["properties"][name], depth + 1)
    return value


def body_for(spec, operation):
    schema = (
        operation.get("requestBody", {})
        .get("content", {})
        .get("application/json", {})
        .get("schema")
    )
    return None if schema is None else resolve(spec, schema)


def required_query(operation, timestamp):
    query = []
    for parameter in operation.get("parameters", []):
        if parameter.get("in") != "query" or not parameter.get("required"):
            continue
        name = parameter["name"]
        kind = parameter.get("schema", {}).get("type", "string")
        if name == "timestamp":
            value = timestamp
        elif kind in ("integer", "number"):
            value = 0 if name == "start" else 1
        else:
            value = "x"
        query.append(f"{name}={value}")
    return query


def concrete_path(path, values):
    for name, value in values.items():
        path = path.replace("{" + name + "}", str(value))
    while "{" in path:
        start = path.index("{")
        end = path.index("}", start)
        path = path[:start] + "x" + path[end + 1 :]
    return path


def share_snapshot(album, share):
    status, body = api(
        "POST",
        "/get/prefetch",
        headers={"x-album-id": album, "x-share-id": share},
        body={},
    )
    if status != 200 or "token" not in body:
        raise SystemExit(f"share snapshot failed: {status} {body!r}")
    return body["token"], body["prefetch"]["timestamp"]


def admin_snapshot():
    status, body = api("POST", "/get/prefetch", headers={"Cookie": f"jwt={ADMIN_JWT}"}, body={})
    if status != 200:
        raise SystemExit(f"admin snapshot failed: {status} {body!r}")
    return body["token"], body["prefetch"]["timestamp"]


def rows_for(token, timestamp):
    status, body = api(
        "GET",
        f"/get/get-data?timestamp={timestamp}&start=0&end=100",
        headers={
            "Authorization": f"Bearer {token}",
            "Cookie": f"jwt={ADMIN_JWT}",
        },
    )
    if status != 200 or not isinstance(body, list):
        raise SystemExit(f"get-data failed: {status} {body!r}")
    return body


def asset_in(rows, album):
    for row in rows:
        abstract = row.get("abstractData", {})
        if abstract.get("type") != "album" and abstract.get("album") == album:
            return abstract.get("id")
    raise SystemExit(f"no asset found in album {album}")


def demoted_cookie():
    api(
        "POST",
        "/post/users/register",
        headers={"Cookie": f"jwt={ADMIN_JWT}"},
        body={"userId": EDITOR, "password": PASSWORD},
    )
    # The session token only exists in Set-Cookie, so read it from the raw
    # response rather than through the JSON helper.
    token = login_token(EDITOR, PASSWORD)
    api(
        "PUT",
        "/put/users/admin",
        headers={"Cookie": f"jwt={ADMIN_JWT}"},
        body={"userId": EDITOR, "admin": False},
    )
    return token


def login_token(user, password):
    data = json.dumps({"userId": user, "password": password}).encode()
    request = urllib.request.Request(BASE + "/post/users/authenticate", data=data, method="POST")
    request.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(request, timeout=30) as response:
        cookie = response.headers.get("Set-Cookie", "")
    for part in cookie.split(";"):
        part = part.strip()
        if part.startswith("jwt="):
            return part[len("jwt=") :]
    raise SystemExit("login returned no jwt cookie")


def build_checks(root: Path) -> list[dict]:
    """The matrix as executable checks: operation, check kind, expectation."""
    rows = auth_rules.load_matrix(root / "backend" / "auth-matrix.json")
    by_key = {(row["method"], row["path"]): row for row in rows}
    checks = []
    for (method, path), check in sorted(
        auth_rules.test_plan(rows).items(), key=lambda item: (item[0][1], item[0][0])
    ):
        checks.append(
            {
                "operationId": by_key[(method, path)]["operationId"],
                "method": method,
                "path": path,
                "check": check,
                "expect": auth_rules.EXPECTED[check],
            }
        )
    return checks


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    plan = build_checks(root)
    spec = json.loads((root / "backend" / "openapi.json").read_text())

    token_a, ts_a = share_snapshot(ALBUM_A, SHARE_A)
    admin_token, admin_ts = admin_snapshot()
    rows = rows_for(admin_token, admin_ts)
    asset_b = asset_in(rows, ALBUM_B)
    demoted = demoted_cookie()

    failures = []
    counts = {}
    for entry in plan:
        method, path, check = entry["method"], entry["path"], entry["check"]
        operation = spec["paths"][path][method.lower()]
        query = required_query(operation, ts_a)
        body = body_for(spec, operation)
        headers = {}
        params = {}

        if check == auth_rules.TEST_CROSS_SCOPE:
            headers["Authorization"] = f"Bearer {token_a}"
            # Present the owning share so the presenter check passes and the
            # album-scope check (not the missing presenter) decides: 404.
            headers["x-album-id"] = ALBUM_A
            headers["x-share-id"] = SHARE_A
            params = {"asset_id": asset_b}
            headers["X-Check"] = check
        elif check == auth_rules.TEST_WRONG_CREDENTIAL:
            headers["Authorization"] = f"Bearer {ADMIN_JWT}"
            headers["X-Check"] = check
        elif check == auth_rules.TEST_RENEW_MISMATCH:
            headers.update({"x-album-id": ALBUM_B, "x-share-id": SHARE_B})
            body = {"token": token_a}
            headers["X-Check"] = check
        elif check == auth_rules.TEST_DEMOTED:
            headers["Cookie"] = f"jwt={demoted}"
            headers["X-Check"] = check
        else:
            continue

        url = concrete_path(path, params)
        if query:
            url += "?" + "&".join(query)
        status, payload = api(method, url, headers=headers, body=body)
        counts[check] = counts.get(check, 0) + 1
        expect = entry["expect"]
        admitted = 200 <= status < 300
        matched = (
            not admitted
            if expect.get("refusal")
            else status in expect.get("statuses", [])
        )
        if not matched:
            failures.append(
                f"{entry['operationId']} [{check}]: expected {expect}, got {status} {payload!r}"
            )

    for check, count in sorted(counts.items()):
        print(f"{check}: {count}")
    if failures:
        print(f"\n{len(failures)} matrix check failure(s):", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1
    print(f"all {sum(counts.values())} matrix checks matched the model")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
