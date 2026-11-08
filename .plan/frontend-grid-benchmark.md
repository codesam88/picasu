---
status: backlog
type: chore
priority: medium
area: frontend
---

## Notes

Reproducible frontend-perceived grid benchmark. Browsing libraries of
large (~5MB) pictures shows perceptible grid lag (slow reload, scroll
jank) even on LAN — but no measurement exists: the bench harness
(`just bench ...`) covers the backend only (scan cells, prefetch
latencies, thumb fetch), never a scripted browser. Do not optimize on
suspicion; build the measurement first.

## Access patterns (one metric each)

- Cold load (fresh profile → `/timeline` → first 20 tiles):
  time-to-first-tiles, total blocking time to interactive grid.
- Scroll sweep (constant velocity through N rows): dropped frames /
  long-task count, late tile pop-in.
- Random jump (scrollbar to 50%/90% offset): time-to-settled-tiles.
- Resize drag (1920→1366→1920 stepped): re-layout wall time, flicker.
- Post-upload reload: refresh-to-tiles-visible (regression-guard for
  the refresh-on-upload behavior).
- Filter change (timeline → album/search): same as cold load, warm.

## Setups

- A — quiescent baseline: seeded repo (~500 × 5MB, like the report),
  fully indexed, then drive UI patterns against the idle server.
- B — during-scan: same repo, drive UI while a rescan runs (extends
  the harness's existing scan-phase probing to the browser).
- C — factor matrix, one variable at a time: image count
  (100/500/2000), original size (small vs 5MB), viewport, DPR 1 vs 2.
- D — run the browser against the LAN IP, not loopback; record
  effective throughput per cell.

## Tools (reuse, don't invent)

Playwright + CDP (trace per pattern, `PerformanceObserver` longtask
entries, rAF deltas, request waterfall) on top of the existing
launcher/seeded-repo/extract pattern, so grid cells emit the same
artifact shape as backend cells. No Lighthouse CI (too coarse for
scroll jank), no new frameworks.

## Hypotheses to kill first

- Original size should not matter for tiles (fixed 720p fetches);
  if 5MB originals do slow the grid, A/B points at scan contention
  or pending-state decodes instead.
- A-vs-B quantifies indexing contention directly.
- Run every pattern cold-cache and warm-cache.

## Build order

Grid workload script (patterns + trace collection) → seeded 5MB repo
→ setup A cell → setup B cell → extractor rows → optimize with
numbers in hand.

## Relations

- Extends `bench-harness-py` (backend cells) to the browser.
- Guards the refresh-on-upload behavior and the fixed row-height
  layout work.
