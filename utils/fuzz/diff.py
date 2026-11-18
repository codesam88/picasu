#!/usr/bin/env python3
"""Differential diff across per-role Schemathesis reports (`just fuzz roles`).

Loads report-<role>.json files, groups findings by operation, and diffs
same-operation outcomes across roles. Fails (exit 1) only on NEW diffs that
are not listed in the baseline file; exits 0 on a clean (fully baselined) run.
Exits 2 on rate/limit errors (BLOCKED).
"""

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_REPORT_DIR = HERE / "out"
DEFAULT_BASELINE = HERE / "baseline-roles.json"
DEFAULT_MATRIX = HERE / "matrix.json"


def load_reports(report_dir):
    reports = {}
    for path in sorted(report_dir.glob("report-*.json")):
        role = path.stem[len("report-"):]
        try:
            reports[role] = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError) as exc:
            print(f"ERROR: cannot read {path}: {exc}", file=sys.stderr)
            sys.exit(1)
    return reports


RATE_NEEDLES = ("429", "rate limit", "rate-limit", "too many requests")


def rate_limit_check(reports, report_dir):
    for role, report in reports.items():
        for err in report.get("errors") or []:
            title = (err.get("title") or "").lower()
            if any(n in title for n in RATE_NEEDLES):
                print(f"BLOCKED: rate/limit error in report {role}: {err.get('title')}", file=sys.stderr)
                sys.exit(2)
    for log_path in report_dir.glob("role-*.log"):
        try:
            text = log_path.read_text(errors="replace").lower()
        except OSError:
            continue
        if any(n in text for n in RATE_NEEDLES):
            print(f"BLOCKED: rate/limit error in {log_path}", file=sys.stderr)
            sys.exit(2)


def op_failure_types(report):
    out = {}
    for failure in report.get("failures") or []:
        ftype = failure.get("type") or failure.get("title") or "unknown"
        for op in failure.get("operations") or []:
            out.setdefault(str(op).strip(), set()).add(str(ftype).strip())
    return out


def op_rejected(report):
    out = {}
    for label, phases in (report.get("valid_rates") or {}).items():
        total = 0
        for rates in (phases or {}).values():
            total += (rates or {}).get("rejected", 0)
        if total:
            out[label] = total
    return out


def role_errors(report):
    return sorted({(e.get("title") or "unknown") for e in (report.get("errors") or [])})


def compute_findings(reports):
    findings = []

    # Role-level errors (triaged separately from per-operation diffs).
    for role in sorted(reports):
        for err in role_errors(reports[role]):
            findings.append(
                {
                    "id": f"role-error::{role}::{err}",
                    "description": f"role {role} reported error: {err}",
                }
            )

    # Per-operation cross-role diff.
    per_role_fail = {role: op_failure_types(r) for role, r in reports.items()}
    per_role_rej = {role: op_rejected(r) for role, r in reports.items()}

    all_ops = set()
    for d in per_role_fail.values():
        all_ops.update(d)
    for d in per_role_rej.values():
        all_ops.update(d)

    for op in sorted(all_ops):
        sigs = {}
        for role in reports:
            has_data = op in per_role_fail[role] or op in per_role_rej[role]
            if not has_data:
                continue
            ft = sorted(per_role_fail[role].get(op, ()))
            sigs[role] = (tuple(ft),)
        if len(sigs) < 2:
            continue
        if len(set(sigs.values())) > 1:
            detail = ", ".join(f"{r}={sigs[r]}" for r in sorted(sigs))
            findings.append(
                {
                    "id": f"op-diff::{op}",
                    "description": f"operation {op} outcome differs across roles: {detail}",
                }
            )

    return findings


def load_baseline_ids(path):
    ids = set()
    if not path.is_file():
        return ids
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        print(f"ERROR: cannot read baseline {path}: {exc}", file=sys.stderr)
        sys.exit(1)
    entries = data.get("findings") if isinstance(data, dict) else data
    for entry in entries or []:
        if isinstance(entry, str):
            ids.add(entry)
        elif isinstance(entry, dict):
            fid = entry.get("id") or entry.get("finding") or entry.get("name")
            if fid is not None:
                ids.add(str(fid))
    return ids


def main():
    parser = argparse.ArgumentParser(description="Differential diff across per-role reports")
    parser.add_argument("--report-dir", default=str(DEFAULT_REPORT_DIR))
    parser.add_argument("--baseline", default=str(DEFAULT_BASELINE))
    parser.add_argument("--matrix", default=str(DEFAULT_MATRIX))
    args = parser.parse_args()

    report_dir = Path(args.report_dir)
    reports = load_reports(report_dir)
    if not reports:
        print(f"ERROR: no report-*.json found in {report_dir}", file=sys.stderr)
        sys.exit(1)

    rate_limit_check(reports, report_dir)
    findings = compute_findings(reports)
    baseline_ids = load_baseline_ids(Path(args.baseline))

    current_ids = {f["id"] for f in findings}
    new_ids = sorted(current_ids - baseline_ids)

    print(f"roles: {', '.join(sorted(reports))}")
    print(f"findings: {len(findings)} total, {len(new_ids)} new")
    for f in findings:
        marker = "NEW" if f["id"] in new_ids else "known"
        print(f"  [{marker}] {f['id']}: {f['description']}")

    if new_ids:
        print("\nNew (unbaselined) diffs:", file=sys.stderr)
        for fid in new_ids:
            print(f"  {fid}", file=sys.stderr)
        sys.exit(1)

    print("OK: no new diffs")
    sys.exit(0)


if __name__ == "__main__":
    raise SystemExit(main())
