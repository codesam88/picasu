#!/usr/bin/env python3
"""Compare a Schemathesis JSON report against the accepted-findings baseline.

Exit 0 iff the tool ran successfully and every reported finding is listed in
the baseline. Exit 1 on new findings or tool breakage (missing report, zero
operations tested, errored cases, incomplete run).
"""

import json
import sys
from pathlib import Path

import yaml


def fail(message):
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)


def main():
    if len(sys.argv) != 3:
        print("usage: check_report.py <report.json> <baseline.yaml>", file=sys.stderr)
        sys.exit(2)
    report_path = Path(sys.argv[1])
    baseline_path = Path(sys.argv[2])

    if not report_path.is_file():
        fail(f"report not found: {report_path}")
    try:
        report = json.loads(report_path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"cannot read report: {exc}")

    if not isinstance(report, dict):
        fail("report is not a JSON object")

    ops = report.get("operations") or {}
    if not isinstance(ops, dict):
        fail("report has no operations summary")
    tested = ops.get("tested", 0)
    selected = ops.get("selected", 0)
    errored = ops.get("errored", 0)
    if selected == 0 or tested == 0:
        fail(f"no operations tested (selected={selected}, tested={tested})")
    if errored and errored > 0:
        fail(f"{errored} errored operation(s)")
    if report.get("errors"):
        fail(f"run reported errors: {report['errors']}")
    if report.get("complete") is False:
        fail("run did not complete")

    # Observed findings as (operation, failure-type) pairs.
    observed = set()
    for failure in report.get("failures") or []:
        ftype = failure.get("type") or failure.get("title") or "unknown"
        ops_list = failure.get("operations")
        if ops_list is None and failure.get("operation") is not None:
            ops_list = [failure["operation"]]
        for op in ops_list or []:
            observed.add((str(op).strip(), str(ftype).strip()))

    # Baseline.
    if not baseline_path.is_file():
        fail(f"baseline not found: {baseline_path}")
    try:
        baseline = yaml.safe_load(baseline_path.read_text()) or {}
    except yaml.YAMLError as exc:
        fail(f"cannot read baseline: {exc}")

    entries = baseline.get("accepted") if isinstance(baseline, dict) else None
    if entries is None:
        # Allow a bare list as well.
        entries = baseline if isinstance(baseline, list) else []
    accepted = set()
    for entry in entries or []:
        if not isinstance(entry, dict):
            continue
        op = entry.get("operation")
        if op is None:
            method = entry.get("method")
            path = entry.get("path")
            if method and path:
                op = f"{method} {path}"
        if op is None:
            continue
        op = str(op).strip()
        failure = entry.get("failure") or entry.get("check")
        if failure:
            accepted.add((op, str(failure).strip()))
        else:
            # An entry with no failure type accepts any failure kind for the
            # operation; entries that name one only accept that kind.
            accepted.add((op, "*"))

    new_findings = sorted(observed - accepted)
    if new_findings:
        print("New (unbaselined) findings:", file=sys.stderr)
        for op, ftype in new_findings:
            print(f"  {op} :: {ftype}", file=sys.stderr)
        fail(f"{len(new_findings)} new finding(s) not in baseline")

    print(
        f"OK: {tested} operations tested, "
        f"{len(observed)} finding(s), all baseline-accepted"
    )


if __name__ == "__main__":
    main()
