---
status: open
type: feature
priority: medium
area: backend
---

## Notes

Write-path load probe + request-path writer migration follow-up to
`worker-pool-rework`. Parent task found: medians show no write-path
difference across eras at N=8, but worst-case columns show multi-second
stalls under full scan load (N=4 edit p99 ≈ 16 s vs median 54 ms).
Medians hide the queueing tail; this task chases it deliberately.

## Problem

Two gaps remain after the pool rework:

1. No harness mode forces sustained DB write load: the current
   edit-rating probe fires 3 writes per probe round from one client.
   Worst-case write latency under a real modify burst (many
   concurrent clients, or one client pipelining dozens of edits)
   during a background scan is unmeasured.
2. Request-path TREE writes bypass the serial writer (deferred
   decision 3 in `worker-pool-rework`): edits contend ad hoc on
   redb's single-writer lock against background flushes. Routing
   them through the writer's Interactive lane would bound each edit
   behind at most one short txn — but costs ~10 router files plus
   `asset_store` API churn, so it needs its own cost/benefit proof.

## Goal

- A write-load harness mode: sustained concurrent metadata edits
  (configurable clients × edits, e.g. `WRITE_CLIENTS`,
  `WRITE_OPS_PER_CLIENT`) against a running scan and against an
  idle server, reporting median + worst-case edit latency plus
  failure/429 counts. Reuse the canonical sets and the existing
  per-round hyperfine + random-fetch probes for the read side.
- A decision, with numbers, on the request-path migration: route
  `PUT /put/edit_*` (+ siblings touching TREE) through the serial
  writer's Interactive lane, or keep direct writes. Success bar is
  worst-case edit latency under scan+burst, not the median.

## Implementation steps

Each step is one worker invocation. Raw outputs stay in tmpfiles
under `/tmp/opencode/write-load-probe/`; only summary tables land
here.

### Step 1 — Write-blast harness mode (no backend changes)

Scope: `utils/bench/harness-a.sh` (+ `extract-bench.py`
worst-column support already exists) only. Add `WRITE_CLIENTS`
(default 1, current behavior) and `WRITE_OPS_PER_CLIENT`
(default 3 = one hyperfine round): N clients fire M edit-rating
PUTs each per probe round (xargs -P), prefetch timestamp reused
from the round; record per-op latency + HTTP code into
`write-blast-round-<n>.tsv`; extractor reports median + worst.
Sanity: `bash -n`, 10-image reuse smoke, write-probe
verification still green. Done when smoke shows blast rows with
non-zero counts and the extractor prints them.

### Step 2 — Baseline blast matrix (current code)

Scope: harness runs only, no backend changes. Matrix on the
canonical set-100, N=8, release: idle server + under-scan ×
blast levels (1×3, 4×10, 8×25), 2 cells each. Record median +
worst edit latency, failure counts, and scan-wall impact vs the
no-blast baseline. Done when raw logs parse and this plan holds
the table; `just plan-lint` passes.

### Step 3 — Request-path migration design (no code)

Scope: design note in this plan only. Specify per-file change
list for routing `PUT /put/edit_*` (+ `assign_album`,
`delete`, `rotate_image`, `regenerate_thumbnail`, `upload`,
`create_dir_album`, `create_share` as decided) through the
writer's Interactive lane with a bounded ack wait + mapped 5xx
on timeout; per-item acks for multi-asset edits; what stays
direct. Done when every touched file is listed with checks.

### Step 4 — Implement + re-measure

Scope: backend per Step 3 list. Done when `just backend-check`
and the full `just backend-test` gate pass, then Step 2 matrix
reruns on the migrated tree. Promote only if worst-case edit
latency under scan+burst improves without regressing scan wall
beyond the known ~8% band. Otherwise close with the numbers.

## Non-goals

- No frontend changes. No new background job types. No change to
  the writer's txns-only contract (short txns; cascades stay in
  the cascade stage).
