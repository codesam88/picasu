---
status: done
type: perf
priority: high
area: tooling
---

## Problem

Probe and storm cells share the request client and, after the sampler
change, the same per-request timing, but not the orchestration. Two
paths encode the same concepts differently:

- `probe_phase` / `run_post_scan`: periodic rounds, a bundle of four
  accesses, one guard per round, sequential;
- `run_storm`: N worker threads, one access, one guard per run, coverage;
- `run_scans` branches on profile name to choose between them, forces
  scan counts, and special-cases idle storms;
- `control` names the probe cell but reads as an idle baseline;
- upload is a profile that runs before the scan, so it cannot be measured
  under a scan background.

The matrix (foreground x background) is the model; the code should be one
executor over that model.

## Target architecture

A cell is a **spec**: an ordered list of phases around an optional populate
scan. A phase is the tuple the model already uses:

```
(background, foregrounds, clients, issue, stop, setup_scan)
```

- `background`: `idle` (no scan during measurement) or `scan`.
- `foregrounds`: one access or a bundle; a foreground is an object with
  `issue(client) -> samples`. Single-request accesses (read, edit, thumb)
  are one timed call; scroll issues its batch; upload issues a file plus a
  visibility wait and returns submit/visible samples.
- `clients`: 1 (sequential rounds) or N (worker threads).
- `issue`: `rounds` (periodic bundle, sequential, guard per round) or
  `stream` (continuous, N workers, guard per run) or `files` (serial upload
  loop).
- `stop`: `scan` terminal, `samples` per access, `ops` per client, `files`.
- `setup_scan`: populate the tree before measuring at rest (idle storm).

One `Executor` runs phases; one `ScanSession` starts a scan, polls to a
terminal state, records wall/status and owns the phase guard. Repeating
scans until a probe-round target becomes a property of the cell spec
(`probe_rounds`), not a profile-name branch.

## Specs (data, not branches)

| cell                          | phases                                                                                                                                          |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `probe`                       | scan: bundle {read, edit, thumb, scroll}, rounds, stop=scan; then idle: same bundle, rounds, stop=samples. Repeats scans to `MIN_PROBE_ROUNDS`. |
| `scan-storm-<kind>-<clients>` | scan: one access, N clients, stream, stop=scan                                                                                                  |
| `idle-storm-<kind>-<clients>` | setup scan; idle: one access, N clients, stream, stop=ops                                                                                       |
| `idle-upload`                 | idle: upload, files, stop=files                                                                                                                 |
| `scan-upload`                 | scan: upload, files, stop=files; samples tagged with scan activity                                                                              |

Upload sources: a fixed deterministic set (`manifests/upload-20.json`,
`UPLOAD_REPO=set-upload`), so every run uploads the same bytes.

## Caller

`run_workload` keeps only: prepare images, resolve the spec from the
profile, run the executor, write the summary. Profile branches disappear
into the spec table; `run_scans`, `probe_phase`, `run_post_scan` and
`run_storm` are replaced by the executor, the scan session and the
foreground objects.

## Validity

- `rounds`: valid rounds / total rounds.
- `stream`: the run guard (storm phase), plus ops done/planned.
- `scan-upload`: files whose submit happened while the scan was active.
- Idle phases: state at start and end.

## Steps

1. Shared sampler: one `issue(access)` function used by probes and storms.
   Done.
2. `ScanSession` + `Executor` with `rounds`/`stream`/`files` issue policies;
   probe, storm and upload ported. Done.
3. Spec table and cell renames (`probe`, `scan-storm-*`, `idle-storm-*`,
   `idle-upload`, `scan-upload`); battery builds from specs. Done.
4. Upload under idle and scan with the fixed image set. Done; each file
   records scan activity at submit and visible.
5. Report validity per phase; docs; re-record. Done.

## Notes

- Upload submits serially. At rest each file waits for its row before the
  next; under a scan the visibility wait is batched after the submissions so
  the whole set overlaps indexing (coverage 100%).
- The at-rest upload waits for the library snapshot to reach the image
  count before starting; `/get/index/status` reports `Idle` before the
  watcher starts, so a settle check alone let the initial index overlap the
  measurement.
- Storm samples now carry their actual background, so the `idle` and `scan`
  rows of one kind are directly comparable.

## Verification

- 50, 100 and 200 images on `8cd567b6`: 15 cells each (probe, six scan
  storms, six idle storms, idle upload, scan upload).
- 45 cells and 1989 baseline rows; each report self-compares with 663 rows
  compared, 0 moved, 0 identity refusals.
- Probe load coverage 100% on all counts; storm coverage 91-93%; upload
  coverage 100% on both backgrounds.
- Idle upload `visible` p50 2.4-2.6 s at every count; scan upload
  `visible` p50 18.2 / 22.1 / 49.8 s at 50/100/200, the burst under the
  initial index.
- 268 tests, ruff and Prettier green.

## Acceptance

- One orchestration path; no profile-name branch in the workload.
- Each cell spec states background, foregrounds, clients, issue, stop.
- Probe/storm numbers unchanged for the same scenario after the port.
- Report rows are matrix points; missing cells are visibly missing.
