"""Unit tests for the auth-matrix derivations used by the Schemathesis hooks.

Run: python3 utils/schemathesis/test_auth_rules.py

The golden expectations pin the derivations against the committed
``backend/auth-matrix.json``: when a backend guard or credential changes, the
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

import auth_rules

REPO = HERE.parents[1]
MATRIX_PATH = REPO / "backend" / "auth-matrix.json"
MATRIX = auth_rules.load_matrix(MATRIX_PATH)


class RealMatrixDerivations(unittest.TestCase):
    def test_share_guarded_ops_are_the_presenter_guarded(self):
        guarded = auth_rules.share_guarded_ops(MATRIX)
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

    def test_test_plan_targets(self):
        plan = auth_rules.test_plan(MATRIX)
        cross_scope = {k for k, v in plan.items() if v == auth_rules.TEST_CROSS_SCOPE}
        renewal = {k for k, v in plan.items() if v == auth_rules.TEST_RENEW_MISMATCH}
        wrong_credential = {k for k, v in plan.items() if v == auth_rules.TEST_WRONG_CREDENTIAL}
        self.assertEqual(cross_scope, {("GET", "/get/metadata/{asset_id}")})
        self.assertEqual(renewal, {("POST", "/post/renew-timestamp-token")})
        self.assertEqual(
            wrong_credential,
            {
                ("GET", "/get/get-data"),
                ("GET", "/get/get-rows"),
                ("GET", "/get/get-scroll-bar"),
                ("POST", "/post/renew-hash-token"),
            },
        )

    def test_test_plan_covers_every_admin_session_op(self):
        plan = auth_rules.test_plan(MATRIX)
        admin_ops = auth_rules.admin_session_ops(MATRIX)
        self.assertIn(("GET", "/get/users"), admin_ops)
        self.assertEqual(len(admin_ops), 22)
        for key in admin_ops:
            self.assertEqual(plan.get(key), auth_rules.TEST_DEMOTED, key)

    def test_test_plan_is_one_check_per_op(self):
        plan = auth_rules.test_plan(MATRIX)
        # No overlap between the four check classes on the real matrix.
        classes = [
            auth_rules.cross_scope_targets(MATRIX),
            auth_rules.renewal_binding_ops(MATRIX),
            auth_rules.wrong_credential_targets(MATRIX),
            auth_rules.admin_session_ops(MATRIX),
        ]
        seen = set()
        for keys in classes:
            self.assertFalse(seen & set(keys), seen & set(keys))
            seen |= set(keys)
        self.assertEqual(set(plan), seen)

    def test_readonly_exempt_matches_guard_mutate(self):
        exempt = auth_rules.readonly_exempt_ops(MATRIX)
        for key in {
            ("POST", "/post/users/authenticate"),
            ("POST", "/post/admin/config/disable-read-only"),
            ("POST", "/post/renew-hash-token"),
            ("POST", "/post/renew-timestamp-token"),
            ("POST", "/post/index/cancel"),
            ("POST", "/get/prefetch"),
        }:
            self.assertIn(key, exempt)
        rows = {auth_rules.op_key(row): row for row in auth_rules.modeled_ops(MATRIX)}
        for key, row in rows.items():
            is_mutating = auth_rules.GUARD_MUTATE in row["x-picasu-auth"]["guards"]
            if key[0] != "GET" and not is_mutating:
                self.assertIn(key, exempt)
            if is_mutating:
                self.assertNotIn(key, exempt)

    def test_cross_scope_expectation_is_404(self):
        # S4: album mismatch on a share-scoped asset read answers 404, not 403.
        self.assertEqual(auth_rules.CROSS_SCOPE_EXPECTED_STATUSES, (404,))


class SyntheticRows(unittest.TestCase):
    @staticmethod
    def row(method, path, guards=(), credentials=(), admin="none", share_scope="n-a", invariants=()):
        return {
            "method": method,
            "path": path,
            "x-picasu-auth": {
                "guards": list(guards),
                "credentials": list(credentials),
                "admin": admin,
                "shareScope": share_scope,
                "invariants": list(invariants),
                "effects": [],
                "public": False,
            },
        }

    def test_non_modeled_rows_are_ignored(self):
        rows = [{"method": "GET", "path": "/{path}", "x-picasu-auth": None}]
        self.assertEqual(auth_rules.share_guarded_ops(rows), frozenset())
        self.assertEqual(auth_rules.test_plan(rows), {})
        self.assertEqual(auth_rules.readonly_exempt_ops(rows), frozenset())

    def test_probe_priority_cross_scope_wins(self):
        # An op matching several classes gets only the highest-priority probe.
        rows = [
            self.row(
                "GET",
                "/get/thing/{asset_id}",
                guards=["GuardPresenter", "GuardSnapshot"],
                credentials=["snapshot", "share", "session"],
                admin="admin",
                share_scope="album",
                invariants=["AUTH-10"],
            )
        ]
        plan = auth_rules.test_plan(rows)
        self.assertEqual(plan[("GET", "/get/thing/{asset_id}")], auth_rules.TEST_CROSS_SCOPE)

    def test_probe_priority_renewal_over_wrong_credential(self):
        rows = [
            self.row(
                "POST",
                "/post/renew",
                guards=["GuardPresenter"],
                credentials=["snapshot", "share"],
                share_scope="album",
                invariants=["AUTH-10"],
            )
        ]
        plan = auth_rules.test_plan(rows)
        self.assertEqual(plan[("POST", "/post/renew")], auth_rules.TEST_RENEW_MISMATCH)

    def test_renewal_requires_album_scope_and_invariant(self):
        rows = [
            self.row(
                "POST",
                "/post/renew-share",
                guards=["GuardPresenter"],
                credentials=["session", "share"],
                share_scope="album",
            ),
            self.row(
                "POST",
                "/post/renew-other",
                guards=["GuardPresenter"],
                credentials=["session", "share"],
                share_scope="album",
                invariants=["AUTH-10"],
            ),
        ]
        self.assertEqual(
            auth_rules.renewal_binding_ops(rows), {("POST", "/post/renew-other")}
        )

    def test_admin_session_ops_require_admin_and_session_credential(self):
        rows = [
            self.row("GET", "/a", guards=["GuardAdmin"], credentials=["session"], admin="admin"),
            self.row("GET", "/b", guards=["GuardUser"], credentials=["session"], admin="any"),
            self.row("GET", "/c", guards=["GuardAdmin"], credentials=["session"], admin="none"),
        ]
        self.assertEqual(auth_rules.admin_session_ops(rows), {("GET", "/a")})


class WrongTypToken(unittest.TestCase):
    def test_prefers_session_for_snapshot_guards(self):
        self.assertEqual(auth_rules.wrong_credential_token(("snapshot",)), "session")

    def test_any_typ_outside_required(self):
        self.assertEqual(auth_rules.wrong_credential_token(("session",)), "share")
        self.assertEqual(auth_rules.wrong_credential_token(("session", "share", "asset")), "snapshot")

    def test_no_requirement_takes_first_vocab_kind(self):
        # Nothing required: any kind works, so the preference order answers.
        self.assertEqual(auth_rules.wrong_credential_token(()), "session")

    def test_none_when_all_typs_required(self):
        self.assertIsNone(auth_rules.wrong_credential_token(auth_rules.CREDENTIAL_VOCAB))


class ScopeMismatchPairs(unittest.TestCase):
    def test_two_pairs_cross_both_ways(self):
        pairs = [("album-a", "share-a"), ("album-b", "share-b")]
        self.assertEqual(
            auth_rules.scope_mismatch_pairs(pairs),
            [(("album-a", "share-a"), ("album-b", "share-b")),
             (("album-b", "share-b"), ("album-a", "share-a"))],
        )

    def test_single_pair_has_no_mismatch(self):
        self.assertEqual(auth_rules.scope_mismatch_pairs([("album-a", "share-a")]), [])


class MatrixLoading(unittest.TestCase):
    def test_rejects_non_list(self):
        with tempfile.NamedTemporaryFile("w", suffix=".json") as handle:
            json.dump({"ops": []}, handle)
            handle.flush()
            with self.assertRaises(ValueError):
                auth_rules.load_matrix(handle.name)


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



class ProbePlan(unittest.TestCase):
    """The matrix as executable checks: operation, check kind, expectation."""

    def test_plan_matches_the_matrix_and_the_document(self):
        import fuzz_matrix

        plan = fuzz_matrix.build_checks(REPO)
        spec = json.loads((REPO / "backend" / "openapi.json").read_text())
        self.assertEqual(len(plan), len(auth_rules.test_plan(MATRIX)))
        for entry in plan:
            operation = spec["paths"][entry["path"]][entry["method"].lower()]
            self.assertEqual(operation["operationId"], entry["operationId"])
            self.assertIn("expect", entry)

    def test_expectations_distinguish_exact_status_from_refusal(self):
        import fuzz_matrix

        plan = {entry["check"]: entry["expect"] for entry in fuzz_matrix.build_checks(REPO)}
        self.assertEqual(plan[auth_rules.TEST_CROSS_SCOPE], {"statuses": [404]})
        self.assertEqual(plan[auth_rules.TEST_WRONG_CREDENTIAL], {"statuses": [401]})
        self.assertEqual(plan[auth_rules.TEST_RENEW_MISMATCH], {"statuses": [401]})
        # Registration also has a public path, so a demoted caller is refused
        # with 403 rather than 401; the claim is "not admitted".
        self.assertEqual(plan[auth_rules.TEST_DEMOTED], {"refusal": True})


if __name__ == "__main__":
    unittest.main()
