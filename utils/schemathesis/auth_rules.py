"""Derive the fuzz harness's auth knowledge from ``backend/auth-matrix.json``.

``backend/auth-matrix.json`` is the committed machine contract of the
per-operation auth model (``x-picasu-auth``: guards, shareScope, credentials,
admin, fallback, invariants, public), generated from the annotated spec and gated by
``openapi-check``. The hooks in this directory consume it through the pure
functions below instead of hand-maintained operation lists, so a guard or
credential change in the backend flows into the checks on the next matrix
regeneration. ``load_matrix`` is the only I/O; every derivation function is
pure and unit-tested in ``test_auth_rules.py``.
"""

from __future__ import annotations

import json

GUARD_PRESENTER = "GuardPresenter"
GUARD_MUTATE = "GuardMutate"

# Credential kinds the backend mints; used to pick a deliberately wrong-credential token
# for a guard.
CREDENTIAL_VOCAB = ("session", "share", "snapshot", "asset")

# AUTH-10: renewal operations re-mint a token bound to the identity that
# owns it.
RENEWAL_INVARIANT = "AUTH-10"

# S4 binding semantics: a share-scoped snapshot token may only read assets of
# its own album; a foreign-album asset answers 404 — not 403 — so the route
# does not reveal that the id exists (get_metadata contract).
CROSS_SCOPE_EXPECTED_STATUSES = (404,)

TEST_CROSS_SCOPE = "cross-scope"
TEST_WRONG_CREDENTIAL = "wrong-credential"
TEST_RENEW_MISMATCH = "renew-mismatch"
TEST_DEMOTED = "demoted"


def load_matrix(path):
    """Read the committed auth matrix; the artifact is part of the repo, so a
    missing or malformed file fails the run instead of degrading to hand
    lists."""
    with open(path, encoding="utf-8") as handle:
        rows = json.load(handle)
    if not isinstance(rows, list):
        raise ValueError(f"auth matrix must be a list of operations: {path}")
    return rows


def modeled_ops(rows):
    """Rows that carry an ``x-picasu-auth`` annotation."""
    return [row for row in rows if row.get("x-picasu-auth")]


def op_key(row):
    return (row["method"].upper(), row["path"])


def _auth(row):
    return row["x-picasu-auth"]


def _credentials(row):
    return tuple(_auth(row).get("credentials") or ())


def share_guarded_ops(rows):
    """Operations GuardPresenter-guarded: presenting the share headers routes
    the guest path on these."""
    return frozenset(
        op_key(row) for row in modeled_ops(rows) if GUARD_PRESENTER in _auth(row)["guards"]
    )


def readonly_exempt_ops(rows):
    """Non-GET operations that must keep answering under read-only mode.

    GuardMutate is the read-only gate (405 while enabled); the non-GET
    operations without it are either side-effect-free (authenticate, token
    renewals), the cancel escape hatch, or the route that disables read-only
    mode itself.
    """
    return frozenset(
        op_key(row)
        for row in modeled_ops(rows)
        if row["method"].upper() != "GET" and GUARD_MUTATE not in _auth(row)["guards"]
    )


def snapshot_typed_ops(rows):
    """Bearer operations whose guard decodes a snapshot-typed token."""
    return [row for row in modeled_ops(rows) if "snapshot" in _credentials(row)]


def cross_scope_targets(rows):
    """Snapshot-typed operations whose path template binds an asset id: the
    cross-scope check sends a share-A snapshot token against an album-B asset
    and expects ``CROSS_SCOPE_EXPECTED_STATUSES``."""
    return frozenset(
        op_key(row) for row in snapshot_typed_ops(rows) if "{" in row["path"]
    )


def wrong_credential_targets(rows):
    """Snapshot-typed operations without an asset-bound path: probing them
    with a credential the operation does not accept exercises the
    credential check alone."""
    return frozenset(op_key(row) for row in snapshot_typed_ops(rows) if "{" not in row["path"])


def renewal_binding_ops(rows):
    """Presenter-guarded renewals of an album-scoped share snapshot (AUTH-10):
    a share-B request must not renew a token minted for share A."""
    return frozenset(
        op_key(row)
        for row in modeled_ops(rows)
        if GUARD_PRESENTER in _auth(row)["guards"]
        and _auth(row).get("shareScope") == "album"
        and RENEWAL_INVARIANT in (_auth(row).get("invariants") or ())
    )


def admin_session_ops(rows):
    """Session-cookie operations requiring the admin role: a demoted user's
    still-live token must lose access on all of them immediately."""
    return frozenset(
        op_key(row)
        for row in modeled_ops(rows)
        if _auth(row).get("admin") == "admin" and "session" in _credentials(row)
    )


# What each check asserts. A status check is exact; a refusal check accepts
# any 4xx, because "the demoted token no longer holds the privilege" is the
# claim — routes with a non-guard admission path (registration, bootstrap)
# refuse in ways other than 401 while those paths are unavailable.
EXPECTED = {
    TEST_CROSS_SCOPE: {"statuses": list(CROSS_SCOPE_EXPECTED_STATUSES)},
    TEST_WRONG_CREDENTIAL: {"statuses": [401]},
    TEST_RENEW_MISMATCH: {"statuses": [401]},
    TEST_DEMOTED: {"refusal": True},
}


def test_plan(rows):
    """Map ``(method, path)`` to the check that operation carries. One check
    per operation, priority: cross-scope > renewal-mismatch > wrong-credential >
    demoted."""
    plan = {}
    for name, keys in (
        (TEST_CROSS_SCOPE, cross_scope_targets(rows)),
        (TEST_RENEW_MISMATCH, renewal_binding_ops(rows)),
        (TEST_WRONG_CREDENTIAL, wrong_credential_targets(rows)),
        (TEST_DEMOTED, admin_session_ops(rows)),
    ):
        for key in keys:
            plan.setdefault(key, name)
    return plan


def wrong_credential_token(required_typs, vocab=CREDENTIAL_VOCAB):
    """First kind in ``vocab`` the operation does not accept, else None."""
    required = set(required_typs or ())
    return next((kind for kind in vocab if kind not in required), None)


def scope_mismatch_pairs(pairs):
    """Cross seeded ``(album_id, share_id)`` fixtures: for every share
    identity, the foreign scope it must not reach, as
    ``(own_pair, foreign_pair)`` tuples of ``(album_id, share_id)``."""
    mismatches = []
    for own in pairs:
        for foreign in pairs:
            if foreign != own:
                mismatches.append((own, foreign))
    return mismatches
