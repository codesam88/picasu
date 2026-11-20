# Benchmark baselines

New baseline set collected on branch `perf/new-baseline` (commit
`3f1040c1df33a7d9f1e0ee99c169e491f497d055`) using the streamlined
battery harness. Raw artifacts: `/tmp/opencode/baselines-new/{micro,gate}/`
(extracts under `/tmp/opencode/baselines-new/extracts/`). Checked-in bands:
`utils/bench/baselines.tsv` (micro + gate `:median` rows).

## Setup

- Branch: `perf/new-baseline`
- Commit: `3f1040c1df33a7d9f1e0ee99c169e491f497d055`
- Host: `codex`, Linux 7.0.0-38-generic, x86_64, 8 logical CPUs, 62 GiB RAM
- CPU: intel_pstate, governor `powersave`, energy_performance_preference
  `balance_power`, max 2.4 GHz, no turbo. This is not the `ondemand`
  governor used for earlier baseline runs; absolute latency figures are
  not directly comparable to those runs.
- Image sets: `~/picasu-bench/set-{50,100,200}` (BENCH_SETS_ROOT default)
- Battery knobs: BATTERY_REPEATS=3, SCAN_REPEATS=2, hyperfine REPEATS=3 /
  WARMUP=1, NCPU=8, BUILD_PROFILE=release, storm CLIENTS=8 / OPS=10 /
  MARGIN_S=0.5
- Co-located load caveat: the host was shared during collection (~33 GiB
  in use, ~30 GiB shared/buff-cache); numbers reflect that load and should
  not be treated as isolated-machine figures.

## Micro battery (linearity check)

Medians over 3 repeats, 2 scans per repeat.

| scale | scan wall s (median) | scan wall range | idle prefetch p50 ms | scan prefetch p50 ms | scroll data p50 ms | tree build p50 us | filter clone p50 us | vm HWM MB | temp_db MB |
| ----: | -------------------: | --------------- | -------------------: | -------------------: | -----------------: | ----------------: | ------------------: | --------: | ---------: |
|    50 |               5.2495 | 5.1495-5.347    |                  7.0 |                  8.1 |                1.0 |             422.5 |               510.5 |     369.4 |        0.6 |
|   200 |               18.061 | 18.053-18.1665  |                  7.1 |                  8.0 |                1.7 |            1588.0 |              2018.5 |     376.8 |        2.2 |

Ranges (min-max across repeat rows):

| scale | idle prefetch p50 ms | scroll data p50 ms | temp_db MB | vm HWM MB   |
| ----: | -------------------- | ------------------ | ---------- | ----------- |
|    50 | 6.7-11.5             | 1.0-1.1            | 0.6-0.6    | 369.0-369.4 |
|   200 | 6.9-11.6             | 1.5-1.9            | 1.1-2.2    | 376.2-378.8 |

## Gate battery (set-100)

Medians over 3 repeats, 2 scans per repeat (storm-idle forces 1 scan).

Scan walls:

| cell             | scan wall s (median) | range         |
| ---------------- | -------------------: | ------------- |
| gate-control     |               9.3855 | 9.3795-9.5155 |
| gate-storm-read  |               9.3895 | 9.386-9.4865  |
| gate-storm-thumb |               9.3815 | 9.3785-9.3825 |
| gate-storm-diff  |               6.2565 | 6.256-6.352   |
| gate-storm-idle  |                9.688 | 9.68-9.892    |
| gate-upload      |              17.5515 | 17.5465-17.56 |

Idle latencies (p50 ms):

| cell             | prefetch | thumb | edit |
| ---------------- | -------: | ----: | ---: |
| gate-control     |     11.6 |  11.5 | 12.4 |
| gate-storm-read  |     11.6 |  12.4 | 12.5 |
| gate-storm-thumb |     11.6 |  12.7 | 12.5 |
| gate-storm-diff  |     11.3 |  12.4 | 12.4 |
| gate-storm-idle  |      6.9 |   7.4 |  7.4 |
| gate-upload      |     11.7 |   8.0 | 13.2 |

Scan latencies (p50 ms; n/a for storm cells that do not run during-scan probes):

| cell             |         prefetch |           thumb |             edit |
| ---------------- | ---------------: | --------------: | ---------------: |
| gate-control     |              8.1 |             3.2 |              8.6 |
| gate-storm-read  |              n/a |             n/a |              n/a |
| gate-storm-thumb |              n/a |             n/a |              n/a |
| gate-storm-diff  |              n/a |             n/a |              n/a |
| gate-storm-idle  |              n/a |             n/a |              n/a |
| gate-upload      | n/a (uscan 13.0) | n/a (uscan 2.6) | n/a (uscan 13.7) |

Upload cell submit/visible p50 ms: submit 3.0, visible 636.0.

Scroll (p50 ms):

| cell             | bar | rows | data |
| ---------------- | --: | ---: | ---: |
| gate-control     | 0.7 |  0.7 |  1.2 |
| gate-storm-read  | 1.3 |  1.5 |  2.4 |
| gate-storm-thumb | 1.4 |  1.5 |  2.4 |
| gate-storm-diff  | 0.9 |  1.3 |  2.1 |
| gate-storm-idle  | 0.6 |  0.7 |  1.2 |
| gate-upload      | 1.3 |  1.4 |  3.2 |

Stages (p50 us):

| cell             | tree_refresh_build | filter_snapshot_clone |
| ---------------- | -----------------: | --------------------: |
| gate-control     |              904.0 |                1132.0 |
| gate-storm-read  |              726.5 |                 832.5 |
| gate-storm-thumb |              516.0 |                 541.5 |
| gate-storm-diff  |              729.0 |                 867.5 |
| gate-storm-idle  |                0.0 |                   0.0 |
| gate-upload      |             1906.0 |                2465.0 |

Resources:

| cell             | vm HWM MB | temp_db MB | index_v5 MB | cache_db MB | compressed MB |
| ---------------- | --------: | ---------: | ----------: | ----------: | ------------: |
| gate-control     |     372.6 |        0.6 |         0.4 |         0.6 |           4.0 |
| gate-storm-read  |     369.3 |        0.6 |         0.7 |         0.6 |           4.0 |
| gate-storm-thumb |     369.3 |        0.6 |         0.6 |         0.6 |           4.0 |
| gate-storm-diff  |     370.1 |        0.6 |         0.7 |         0.6 |           4.0 |
| gate-storm-idle  |     365.8 |        1.1 |         0.7 |         1.1 |           4.0 |
| gate-upload      |     391.1 |        8.9 |         0.7 |         2.2 |           4.0 |

Ranges (min-max across repeat rows) for headline metrics:

| cell             | scan wall s   | idle prefetch p50 ms | scroll data p50 ms | temp_db MB | vm HWM MB   |
| ---------------- | ------------- | -------------------- | ------------------ | ---------- | ----------- |
| gate-control     | 9.3795-9.5155 | 6.9-11.8             | 1.2-1.3            | 0.6-0.6    | 372.6-372.9 |
| gate-storm-read  | 9.386-9.4865  | 11.4-11.8            | 2.3-2.5            | 0.6-0.6    | 368.8-369.5 |
| gate-storm-thumb | 9.3785-9.3825 | 11.6-11.7            | 1.3-2.4            | 0.6-0.6    | 369.1-369.5 |
| gate-storm-diff  | 6.256-6.352   | 11.1-12.2            | 1.5-2.4            | 0.6-0.6    | 370.0-370.3 |
| gate-storm-idle  | 9.68-9.892    | 6.8-6.9              | 1.2-1.2            | 0.6-1.1    | 365.8-365.8 |
| gate-upload      | 17.5465-17.56 | 6.9-11.7             | 2.1-3.3            | 8.9-17.8   | 388.4-394.1 |

## Scale (5650) still needs

Not run (hours of wall time). Follow-up work: run `just bench battery scale`
(set-5650, control + storm-diff + upload) and record its `:median` rows.
Open perf items that this baseline set does not yet cover:

- Edit-path slowness at 5650 rows (barrier/flush path) and larger libraries.
- temp_db.redb growth under probe load (one table per prefetch cache miss).
- Retention/lag at scale (writer-cascade lag cap).
- 50k-image set is not available (~27 GB); 5650 remains the largest
  measured scale.

Scroll/resource columns and range columns remain ungated (informational)
in `baselines.tsv` by design; gate on them only after a confirming run.
