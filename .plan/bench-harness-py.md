---
status: in-progress
type: chore
area: backend
priority: high
---

## Problem

`utils/bench/harness-a.sh` (631 lines bash) owns the benchmark protocol
behind every plan measurement, but process control, stuck detection,
diagnostics capture, and JSON handling are painful in bash. The Python
`launch-cell.py` wrapper mitigates from outside; protocol internals
(status polling without stall detection, no watermark trail, ad-hoc
artifacts) remain bash-bound.

## Goal

Two layers, built in order:

1. Backend launcher (this plan): one place that sets up backend state,
   spawns and supervises throwaway backend runs for bench and debug use,
   detects common failures, and captures diagnostics. `serve.sh` stays
   untouched for persistent fuzz/dev use.
2. Workload wrapper (done): `utils/bench/run-cell.py` + `workload.py`
   run desired workloads against a launcher-provided server and capture
   results. `harness-a.sh` retired with the wrapper in place.

Scope decisions (2026-11-04): bench + debug first; supervise-only
(spawn, stream logs, liveness, diag bundle, cleanup — no mid-run
interaction); the Python stack (`run-cell.py` + `workload.py`) replaced
`harness-a.sh` outright.

## Non-goals

## Non-goals

- No backend changes in this plan (diagnostic env stays as-is).
- No reinterpretation of historical cells; the extractor keeps parsing
  old artifacts.

## Steps

1. Contract survey: every env var, endpoint call, artifact file, and
   log line of `harness-a.sh` + `extract-bench.py` that downstream
   depends on (done 2026-11-04, worker report in session log).
2. Backend launcher: `utils/bench/backend_launcher.py` (stdlib only)
   - CLI `utils/bench/launch-backend.py`. Owns state setup (fresh
     wipe with guards vs explicit reuse), port preflight (never hijack),
     spawn (process group, log streaming), readiness
     (`/get/server-info` poll, bootstrap/auth with fresh-vs-existing
     handling), liveness supervision, debug support (bench/debug modes,
     RUST_LOG passthrough, gdb stack capture into
     diag bundle), and `launcher-info.json` seam (base URL, cookie jar,
     pid, out) for the future workload wrapper. Adapts
     `utils/bench/tests/` to the new module; removes `launch-cell.py`.
3. Stub-server test suite: fresh/reuse/bootstrap-closed/port-busy/
   server-dies/ready-timeout paths (no real server needed).
4. Cut over one debug run and one bench cell to the launcher
   (done — Python stack validated against harness cells, then cut over).
5. Retire `harness-a.sh` (done — removed once the workload wrapper
   proved metric-compatible).

## Acceptance

- `python3 -m unittest discover -s utils/bench/tests` green, incl.
  new protocol tests (no real server needed).
- New harness completes a canonical cell; extractor table matches
  hand-checked values; manifest + watermark TSV present.
- Old cells (`bench-cmp-*`) still parse.
