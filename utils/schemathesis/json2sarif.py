#!/usr/bin/env python3
"""Convert a Schemathesis JSON report to SARIF 2.1.0.

Usage:
    json2sarif.py <report.json> <baseline.yaml> <out.sarif>

Findings accepted by the baseline (same operation + failure-type matching as
check_report.py) are excluded, so the Security tab shows only actionable
findings. Advisory channel: every result is emitted at level "warning".

Operation anchoring reads backend/openapi.json relative to the repo root and
locates the path/method declaration line (approximate, line-based). Fingerprints
are built from operation + failure type so GitHub dedups across nightly runs.
"""

import json
import sys
from pathlib import Path

import yaml

OPENAPI_PATH = Path("backend/openapi.json")


def _accepted_set(baseline_path):
    accepted = set()
    if not baseline_path.is_file():
        return accepted
    try:
        baseline = yaml.safe_load(baseline_path.read_text()) or {}
    except yaml.YAMLError:
        return accepted
    entries = baseline.get("accepted") if isinstance(baseline, dict) else None
    if entries is None:
        entries = baseline if isinstance(baseline, list) else []
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
            accepted.add((op, "*"))
    return accepted


def _operation_lines():
    """Map (path, method) to a 1-based line number in backend/openapi.json."""
    mapping = {}
    if not OPENAPI_PATH.is_file():
        return mapping
    text = OPENAPI_PATH.read_text()
    lines = text.splitlines()
    # Find each path key line, then the immediately following method key.
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith('"') and '": {' in stripped and stripped.startswith('"/'):
            # candidate path key
            try:
                path = json.loads(stripped.split('":', 1)[0] + '"')
            except (json.JSONDecodeError, ValueError):
                i += 1
                continue
            # look ahead for method keys
            j = i + 1
            while j < n and lines[j].strip() == "":
                j += 1
            # collect consecutive method keys
            while j < n:
                ms = lines[j].strip()
                if ms == "}" or ms.startswith('"') is False:
                    break
                if '": {' in ms or '":{' in ms:
                    try:
                        method = json.loads(ms.split('":', 1)[0] + '"')
                    except (json.JSONDecodeError, ValueError):
                        break
                    if method.lower() in ("get", "post", "put", "delete", "patch", "head", "options"):
                        mapping[(path, method.upper())] = j + 1
                else:
                    break
                j += 1
                while j < n and lines[j].strip() == "":
                    j += 1
            i = j
        else:
            i += 1
    return mapping


def main():
    if len(sys.argv) != 4:
        print("usage: json2sarif.py <report.json> <baseline.yaml> <out.sarif>", file=sys.stderr)
        sys.exit(2)
    report_path = Path(sys.argv[1])
    baseline_path = Path(sys.argv[2])
    out_path = Path(sys.argv[3])

    report = json.loads(report_path.read_text())
    accepted = _accepted_set(baseline_path)
    lines = _operation_lines()

    version = report.get("schemathesis_version") or report.get("version") or "0.0.0"

    # Collect unique failure types for rules.
    failure_types = []
    seen_types = set()
    # Build new (operation, failure_type) findings.
    findings = []  # list of (op, ftype, title)
    for failure in report.get("failures") or []:
        ftype = str(failure.get("type") or failure.get("title") or "unknown").strip()
        title = str(failure.get("title") or ftype).strip()
        if ftype not in seen_types:
            seen_types.add(ftype)
            failure_types.append((ftype, title))
        ops_list = failure.get("operations")
        if ops_list is None and failure.get("operation") is not None:
            ops_list = [failure["operation"]]
        for op in ops_list or []:
            op = str(op).strip()
            if (op, ftype) in accepted:
                continue
            if (op, "*") in accepted:
                continue
            findings.append((op, ftype, title))

    rules = []
    for ftype, title in failure_types:
        # only include rules that have at least one non-baselined finding
        if any(f[1] == ftype for f in findings):
            rules.append({
                "id": ftype,
                "shortDescription": {"text": title},
            })

    results = []
    for op, ftype, title in findings:
        # parse method/path
        parts = op.split(" ", 1)
        method = parts[0].upper() if len(parts) == 2 else "GET"
        path = parts[1] if len(parts) == 2 else op
        line_no = lines.get((path, method), 1)
        fingerprint = f"{op}|{ftype}|{title}"
        results.append({
            "level": "warning",
            "ruleId": ftype,
            "message": {"text": f"{title} on {op}"},
            "locations": [
                {
                    "physicalLocation": {
                        "artifactLocation": {"uri": "backend/openapi.json"},
                        "region": {"startLine": line_no},
                    }
                }
            ],
            "partialFingerprints": {
                "schemathesis/v1": fingerprint,
            },
        })

    sarif = {
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "Schemathesis",
                        "version": version,
                        "rules": rules,
                    }
                },
                "results": results,
            }
        ],
    }

    out_path.write_text(json.dumps(sarif, indent=2) + "\n")
    print(f"wrote {out_path}: {len(results)} result(s)")


if __name__ == "__main__":
    main()
