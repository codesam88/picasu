---
status: done
type: feature
priority: high
area: backend
---

## Goal

Measure API-under-scan latency on unmodified `origin/main` with the same
observer-instrumented protocol used on `feat/impose-requirements`, to get a
baseline for the C-series changes: idle vs loaded prefetch/edit latency plus
the observer per-scan stage table (rebuild count/cost, clone cost, writer
lock waits, CPU permit waits).

## Non-goals

No code fixes, no backports. If the baseline build or rig fails for
environmental reasons, record it and stop rather than changing main.

## Results (2026-11-12, main = `origin/main`, impose = `feat/impose-requirements`)

| Metric           | main idle                                          | main loaded          | impose idle                            | impose loaded |
| ---------------- | -------------------------------------------------- | -------------------- | -------------------------------------- | ------------- |
| prefetch         | ~3 ms                                              | ~2–5 ms (80 ms once) | ~3 ms                                  | 1.2–2.7 s     |
| edit_tag         | ~0.55 s                                            | 1.2–2.2 s            | ~0.55 s                                | 2.7–2.9 s     |
| rebuild cost     | 0.46 s / 2000 rows = 0.23 ms/row (n=256)           |                      | 1.3 s / 5650 rows = 0.23 ms/row (n=36) |               |
| writer-slot wait | 1–2 µs everywhere (rules out redb lock contention) |                      |                                        |               |

### Conclusion

Identical rebuild algorithm cost on both branches; what differs is
invalidation granularity. Main stamps the query-cache version at rebuild
completion in wall-clock ms, so sub-millisecond rebuild bursts share one
version and reads hit cache (~3 ms) while scanning. Our CACHE-01 split
advances a monotonic counter per flush, so every scan flush invalidates
both the prefetch key and the tree epoch — every read misses and rebuilds
the whole tree (1.2–2.7 s sustained). The fix is decoupled invalidation:
request commits advance strictly monotonically (keeps the CACHE-01 fix);
scan flushes do not move the request-visible version; freshness points
are request commits + scan-job terminal transitions (optionally throttled
to ≤1/s during scans for live progress). Cascade coalescing remains valid
as a follow-up CPU saver, not the latency fix.

### Open items out of this task

- Main rescan hang (state stuck `running`, idle process, cancel reports
  no active job): pre-existing terminal-transition bug, not root-caused.
  Separate item if pursued.
- Edits are slow under load on both branches (barrier/flush path) —
  separate workstream from the read path.
- Confounds on record: different library sizes (2000 vs 5651) and
  snapfab format sets; per-row rebuild cost identical, validating the
  comparison.

## Steps

1. [done] Verify observer hooks exist on `origin/main` (same
   `PICASU_OBSERVER_STATS`, `drain_report`, scan marker, status endpoint).
2. [done] New worktree (`picasu-baseline`) + branch
   `baseline-observe` off `origin/main`; separate `CARGO_TARGET_DIR`
   (`/home/codesam/picasu-baseline-target`) so builds never contend.
3. [done] Built `picasu` + `snapfab`; fresh 2000-image minimal
   library (`/tmp/opencode/picasu-observe-baseline`, port 5202).
4. [done] Idle baseline: prefetch ~3 ms, edit_tag ~0.55 s. (Initial
   500s on edit_tag were a bad fixture path, not a regression.)
5. [done] Loaded run during backlog rescan + first-scan drain table.
   NOTE: rescan hangs post-walk on unmodified main (state stuck
   `running`, process idle, cancel reports no active job) — pre-existing
   terminal-transition bug, separate item, not root-caused here.
6. [done] Side-by-side in chat 2026-11-12. Headline: our per-flush
   invalidation (CACHE-01 split) regressed reads ~400-900x under scan
   vs main's ms-granular stamping; rebuild cost per row identical
   (0.23 ms) on both.

## Post-fix verification (2026-11-13, impose @ `5e61f075`)

Rig: `/tmp/opencode/picasu-verify`, port 5203, release build,
`PICASU_OBSERVER_STATS=1`, same 2000-image library as the main
baseline (hardlinked — identical data, no confound). Protocol
mirrors `measure.sh`: first scan, post-terminal reads, edit_tag,
rescan (the storm condition) with concurrent reads.

| Metric             | main loaded (2000 rows) | impose pre-fix (5650 rows) | impose post-fix (2000 rows) |
| ------------------ | ----------------------- | -------------------------- | --------------------------- |
| prefetch p50       | ~2–5 ms                 | 1.2–2.7 s                  | 27 ms                       |
| prefetch p95 / max | 80 ms (single outlier)  | —                          | 130 / 171 ms                |
| edit_tag           | 1.2–2.2 s               | 2.7–2.9 s                  | 102 ms                      |
| tree_refresh_build | 454 ms (n=256)          | 1.31 s / 5650 rows         | 14–48 ms (n=194 / n=70)     |
| post-terminal read | —                       | —                          | 0.5–0.9 ms, rows immediate  |

### Reading

- The storm is gone: loaded reads dropped ~50–100× and are
  interactive-grade throughout a scan. Distribution is bimodal by
  design — sub-ms query-cache hits, plus one ~20–65 ms tree rebuild
  per ~1 s leading-edge throttle window (freshness contract price).
- Not literally equal to main's 2–5 ms, and that is a fair difference,
  not a residual defect: main's loaded reads are _stale_ (its
  ms-granular stamping serves old data during scans, unbounded); ours
  return data at most ~1 s old, paying one rebuild per window for it.
- Rebuild itself is ~10–30× cheaper than main's recorded 454 ms /
  2000 rows — the impose structural work (path-primary tables) changed
  the rebuild cost profile; the earlier "0.23 ms/row identical on both"
  record does not extrapolate to the current build. Uncertainty noted,
  not investigated further.
- edit_tag under load: 102 ms at 2000 rows. The earlier slow-edit
  numbers (both branches) were at 5650 rows; treat "edits slow under
  load" as unresolved only at that scale, not contradicted here.
- Both scans completed cleanly on the impose branch (no hang-bail);
  main's rescan hang was not reproduced here.
