"""Unit tests for the auth-matrix derivations used by the Schemathesis hooks.

Run: python3 utils/schemathesis/test_auth_matrix.py

The golden expectations pin the derivations against the committed
``backend/auth-matrix.json``: when a backend guard or token-typ changes, the
matrix regeneration makes these tests fail, which prompts a review of the
affected probe instead of letting it drift silently.
"""

import json
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import auth_matrix

REPO = HERE.parents[1]
MATRIX_PATH = REPO / "backend" / "auth-matrix.json"
MATRIX = auth_matrix.load_matrix(MATRIX_PATH)


class RealMatrixDerivations(unittest.TestCase):
    def test_share_guarded_ops_are_the_presenter_guarded(self):
        guarded = auth_matrix.share_guarded_ops(MATRIX)
        self.assertEqual(
            guarded,
            {
                ("POST", "/get/prefetch"),
                ("GET", "/get/config"),
                ("GET", "/object/compressed/{file_path}"),
                ("GET", "/object/imported/{file_path}"),
                ("POST", "/post/renew-timestamp-token"),
            },
        )
        # GuardUser mutations are session-only; they must not receive share headers.
        self.assertNotIn(("PUT", "/put/set_album_title"), guarded)
        self.assertNotIn(("PUT", "/put/set_user_defined_description"), guarded)

    def test_probe_plan_targets(self):
        plan = auth_matrix.probe_plan(MATRIX)
        cross_scope = {k for k, v in plan.items() if v == auth_matrix.PROBE_CROSS_SCOPE}
        renewal = {k for k, v in plan.items() if v == auth_matrix.PROBE_RENEW_MISMATCH}
        wrong_typ = {k for k, v in plan.items() if v == auth_matrix.PROBE_WRONG_TYP}
        self.assertEqual(cross_scope, {("GET", "/get/metadata/{asset_id}")})
        self.assertEqual(renewal, {("POST", "/post/renew-timestamp-token")})
        self.assertEqual(
            wrong_typ,
            {
                ("GET", "/get/get-data"),
                ("GET", "/get/get-rows"),
                ("GET", "/get/get-scroll-bar"),
                ("POST", "/post/renew-hash-token"),
            },
        )

    def test_probe_plan_covers_every_admin_session_op(self):
        plan = auth_matrix.probe_plan(MATRIX)
        admin_ops = auth_matrix.admin_session_ops(MATRIX)
        self.assertIn(("GET", "/get/users"), admin_ops)
        self.assertEqual(len(admin_ops), 22)
        for key in admin_ops:
            self.assertEqual(plan.get(key), auth_matrix.PROBE_DEMOTED, key)

    def test_probe_plan_is_one_probe_per_op(self):
        plan = auth_matrix.probe_plan(MATRIX)
        # No overlap between the four probe classes on the real matrix.
        classes = [
            auth_matrix.cross_scope_targets(MATRIX),
            auth_matrix.renewal_binding_ops(MATRIX),
            auth_matrix.wrong_typ_targets(MATRIX),
            auth_matrix.admin_session_ops(MATRIX),
        ]
        seen = set()
        for keys in classes:
            self.assertFalse(seen & set(keys), seen & set(keys))
            seen |= set(keys)
        self.assertEqual(set(plan), seen)

    def test_readonly_exempt_matches_guard_mutate(self):
        exempt = auth_matrix.readonly_exempt_ops(MATRIX)
        for key in {
            ("POST", "/post/users/authenticate"),
            ("POST", "/post/admin/config/disable-read-only"),
            ("POST", "/post/renew-hash-token"),
            ("POST", "/post/renew-timestamp-token"),
            ("POST", "/post/index/cancel"),
            ("POST", "/get/prefetch"),
        }:
            self.assertIn(key, exempt)
        rows = {auth_matrix.op_key(row): row for row in auth_matrix.modeled_ops(MATRIX)}
        for key, row in rows.items():
            is_mutating = auth_matrix.GUARD_MUTATE in row["x-picasu-auth"]["guards"]
            if key[0] != "GET" and not is_mutating:
                self.assertIn(key, exempt)
            if is_mutating:
                self.assertNotIn(key, exempt)

    def test_cross_scope_expectation_is_404(self):
        # S4: album mismatch on a share-scoped asset read answers 404, not 403.
        self.assertEqual(auth_matrix.CROSS_SCOPE_EXPECTED_STATUSES, (404,))


class SyntheticRows(unittest.TestCase):
    @staticmethod
    def row(method, path, guards=(), token_typ=(), admin="none", share_scope="n-a", invariants=()):
        return {
            "method": method,
            "path": path,
            "x-picasu-auth": {
                "guards": list(guards),
                "tokenTyp": list(token_typ),
                "admin": admin,
                "shareScope": share_scope,
                "invariants": list(invariants),
                "effects": [],
                "public": False,
            },
        }

    def test_non_modeled_rows_are_ignored(self):
        rows = [{"method": "GET", "path": "/{path}", "x-picasu-auth": None}]
        self.assertEqual(auth_matrix.share_guarded_ops(rows), frozenset())
        self.assertEqual(auth_matrix.probe_plan(rows), {})
        self.assertEqual(auth_matrix.readonly_exempt_ops(rows), frozenset())

    def test_probe_priority_cross_scope_wins(self):
        # An op matching several classes gets only the highest-priority probe.
        rows = [
            self.row(
                "GET",
                "/get/thing/{asset_id}",
                guards=["GuardPresenter", "GuardSnapshot"],
                token_typ=["snapshot", "share", "session"],
                admin="admin",
                share_scope="album",
                invariants=["AUTH-10"],
            )
        ]
        plan = auth_matrix.probe_plan(rows)
        self.assertEqual(plan[("GET", "/get/thing/{asset_id}")], auth_matrix.PROBE_CROSS_SCOPE)

    def test_probe_priority_renewal_over_wrong_typ(self):
        rows = [
            self.row(
                "POST",
                "/post/renew",
                guards=["GuardPresenter"],
                token_typ=["snapshot", "share"],
                share_scope="album",
                invariants=["AUTH-10"],
            )
        ]
        plan = auth_matrix.probe_plan(rows)
        self.assertEqual(plan[("POST", "/post/renew")], auth_matrix.PROBE_RENEW_MISMATCH)

    def test_renewal_requires_album_scope_and_invariant(self):
        rows = [
            self.row(
                "POST",
                "/post/renew-share",
                guards=["GuardPresenter"],
                token_typ=["session", "share"],
                share_scope="album",
            ),
            self.row(
                "POST",
                "/post/renew-other",
                guards=["GuardPresenter"],
                token_typ=["session", "share"],
                share_scope="album",
                invariants=["AUTH-10"],
            ),
        ]
        self.assertEqual(
            auth_matrix.renewal_binding_ops(rows), {("POST", "/post/renew-other")}
        )

    def test_admin_session_ops_require_admin_and_session_typ(self):
        rows = [
            self.row("GET", "/a", guards=["GuardAdmin"], token_typ=["session"], admin="admin"),
            self.row("GET", "/b", guards=["GuardUser"], token_typ=["session"], admin="any"),
            self.row("GET", "/c", guards=["GuardAdmin"], token_typ=["session"], admin="none"),
        ]
        self.assertEqual(auth_matrix.admin_session_ops(rows), {("GET", "/a")})


class WrongTypToken(unittest.TestCase):
    def test_prefers_session_for_snapshot_guards(self):
        self.assertEqual(auth_matrix.wrong_typ_token(("snapshot",)), "session")

    def test_any_typ_outside_required(self):
        self.assertEqual(auth_matrix.wrong_typ_token(("session",)), "share")
        self.assertEqual(auth_matrix.wrong_typ_token(("session", "share", "asset")), "snapshot")

    def test_no_requirement_takes_first_vocab_typ(self):
        # Nothing required: any typ works, so the preference order answers.
        self.assertEqual(auth_matrix.wrong_typ_token(()), "session")

    def test_none_when_all_typs_required(self):
        self.assertIsNone(auth_matrix.wrong_typ_token(auth_matrix.TOKEN_TYP_VOCAB))


class ScopeMismatchPairs(unittest.TestCase):
    def test_two_pairs_cross_both_ways(self):
        pairs = [("album-a", "share-a"), ("album-b", "share-b")]
        self.assertEqual(
            auth_matrix.scope_mismatch_pairs(pairs),
            [(("album-a", "share-a"), ("album-b", "share-b")),
             (("album-b", "share-b"), ("album-a", "share-a"))],
        )

    def test_single_pair_has_no_mismatch(self):
        self.assertEqual(auth_matrix.scope_mismatch_pairs([("album-a", "share-a")]), [])


class MatrixLoading(unittest.TestCase):
    def test_rejects_non_list(self):
        with tempfile.NamedTemporaryFile("w", suffix=".json") as handle:
            json.dump({"ops": []}, handle)
            handle.flush()
            with self.assertRaises(ValueError):
                auth_matrix.load_matrix(handle.name)


class DestructiveDisableConsistency(unittest.TestCase):
    """campaign.toml/check.toml disables stay hand-tuned, but their
    operation ids must exist in the matrix (catches renames/removals)."""

    def disabled_ids(self, toml_path):
        with open(toml_path, "rb") as handle:
            config = tomllib.load(handle)
        return {
            op["include-operation-id"]
            for op in config.get("operations", [])
            if not op.get("enabled", True) and "include-operation-id" in op
        }

    def test_disabled_operation_ids_exist_in_matrix(self):
        matrix_ids = {row.get("operationId") for row in MATRIX}
        for toml_path in (REPO / "utils/schemathesis/campaign.toml",
                          REPO / "utils/schemathesis/check.toml"):
            disabled = self.disabled_ids(toml_path)
            self.assertTrue(disabled, toml_path)
            self.assertEqual(disabled - matrix_ids, set(), toml_path)


if __name__ == "__main__":
    unittest.main()
