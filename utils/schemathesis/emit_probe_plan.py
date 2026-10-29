"""Emit the committed probe plan from backend/auth-matrix.json.

The plan is the executable form of the campaign's A1-A5 probes: one entry per
operation the model assigns a probe to, with the statuses that probe must see.
Schemathesis and the deterministic runner both read this artifact, so a guard
or token-typ change reaches both through the matrix rather than through a hand
list.

Usage: python3 utils/schemathesis/emit_probe_plan.py [repo-root]
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import auth_matrix  # noqa: E402  (path set above)

# What each probe asserts. A status check is exact; a refusal check accepts any
# 4xx, because "the demoted token no longer holds the privilege" is the claim —
# routes with an additional public path (registration) refuse with 403 rather
# than 401.
EXPECTED = {
    auth_matrix.PROBE_CROSS_SCOPE: {"statuses": list(auth_matrix.CROSS_SCOPE_EXPECTED_STATUSES)},
    auth_matrix.PROBE_WRONG_TYP: {"statuses": [401]},
    auth_matrix.PROBE_RENEW_MISMATCH: {"statuses": [401]},
    auth_matrix.PROBE_DEMOTED: {"refusal": True},
}


def build(root: Path) -> list[dict]:
    rows = auth_matrix.load_matrix(root / "backend" / "auth-matrix.json")
    by_op = {row["operationId"]: row for row in rows}
    plan = []
    for (method, path), probe in sorted(
        auth_matrix.probe_plan(rows).items(), key=lambda item: (item[0][1], item[0][0])
    ):
        operation = next(
            row
            for row in rows
            if row["method"] == method and row["path"] == path
        )
        plan.append(
            {
                "operationId": operation["operationId"],
                "method": method,
                "path": path,
                "probe": probe,
                "expect": EXPECTED[probe],
            }
        )
    missing = [row["operationId"] for row in rows if row.get("x-picasu-auth") and row["operationId"] not in by_op]
    if missing:
        raise SystemExit(f"auth matrix rows lost their operation id: {missing}")
    return plan


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    plan = build(root)
    out = root / "backend" / "probe-plan.json"
    out.write_text(json.dumps(plan, indent=2) + "\n")
    print(f"probe-plan: wrote {len(plan)} entries to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
