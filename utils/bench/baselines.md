# Benchmark baselines

## Run environment

Host and build of the recorded runs. Compare only runs from the same machine and build: `16n` client counts are resolved from the recorded CPU count.

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 1775/2400 MHz, governor powersave, turbo disabled
- **memory**: 63 GiB
- **load average (1/5/15 min)**: 7.77/8.64/6.68 at start, 6.58/8.18/6.87 at end
- **git**: perf/new-baseline2 @ edbf1d5360b7
- **captured**: 2026-11-22T14:29:09+00:00 to 2026-11-22T14:31:58+00:00

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 1938/2400 MHz, governor powersave, turbo disabled
- **memory**: 63 GiB
- **load average (1/5/15 min)**: 6.07/7.04/5.43 at start, 7.77/8.64/6.68 at end
- **git**: perf/new-baseline2 @ edbf1d5360b7
- **captured**: 2026-11-22T14:24:13+00:00 to 2026-11-22T14:29:09+00:00

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 2400/2400 MHz, governor powersave, turbo disabled
- **memory**: 63 GiB
- **load average (1/5/15 min)**: 3.60/6.92/5.13 at start, 6.07/7.04/5.43 at end
- **git**: perf/new-baseline2 @ edbf1d5360b7
- **captured**: 2026-11-22T14:22:04+00:00 to 2026-11-22T14:24:13+00:00

## Measurement validity

Probe rounds are requests issued and completed while a scan ran; `probe coverage` is the share of scan wall they cover and `storm coverage` the share covered by a concurrent burst. Low coverage or a non-zero `invalid` count means the latency rows describe only part of the scan; `failed` counts operations that did not return 200.

### 50 images

| profile             | reps | valid rounds | probe coverage % | storm coverage % | warmup | invalid | failed |
| :------------------ | ---: | -----------: | ---------------: | ---------------: | :----- | ------: | -----: |
| control             |    2 |           32 |             19.5 |              n/a | true   |       0 |    n/a |
| storm-edit-16n      |    1 |            0 |              n/a |             58.9 | n/a    |       0 |      0 |
| storm-edit-8        |    1 |            0 |              n/a |             61.2 | n/a    |       0 |      0 |
| storm-idle-read-16n |    1 |            0 |              n/a |              n/a | n/a    |       0 |      0 |
| storm-idle-read-8   |    1 |            0 |              n/a |              n/a | n/a    |       0 |      0 |
| storm-read-16n      |    1 |            0 |              n/a |             58.1 | n/a    |       0 |      0 |
| storm-read-8        |    1 |            0 |              n/a |             69.5 | n/a    |       0 |      0 |
| storm-thumb-16n     |    1 |            0 |              n/a |             58.1 | n/a    |       0 |      0 |
| storm-thumb-8       |    1 |            0 |              n/a |             68.5 | n/a    |       0 |      0 |
| upload              |    1 |           22 |             19.5 |              n/a | true   |       0 |    n/a |

### 100 images

| profile             | reps | valid rounds | probe coverage % | storm coverage % | warmup | invalid | failed |
| :------------------ | ---: | -----------: | ---------------: | ---------------: | :----- | ------: | -----: |
| control             |    1 |           30 |             15.8 |              n/a | true   |       0 |    n/a |
| storm-edit-16n      |    1 |            0 |              n/a |             75.9 | n/a    |       0 |      0 |
| storm-edit-8        |    1 |            0 |              n/a |             80.5 | n/a    |       0 |      0 |
| storm-idle-read-16n |    1 |            0 |              n/a |              n/a | n/a    |       0 |      0 |
| storm-idle-read-8   |    1 |            0 |              n/a |              n/a | n/a    |       0 |      0 |
| storm-read-16n      |    1 |            0 |              n/a |             76.9 | n/a    |       0 |      0 |
| storm-read-8        |    1 |            0 |              n/a |             81.9 | n/a    |       0 |      0 |
| storm-thumb-16n     |    1 |            0 |              n/a |             76.4 | n/a    |       0 |      0 |
| storm-thumb-8       |    1 |            0 |              n/a |             82.0 | n/a    |       0 |      0 |
| upload              |    1 |           36 |             20.1 |              n/a | true   |       0 |    n/a |

### 200 images

| profile             | reps | valid rounds | probe coverage % | storm coverage % | warmup | invalid | failed |
| :------------------ | ---: | -----------: | ---------------: | ---------------: | :----- | ------: | -----: |
| control             |    1 |           56 |             21.4 |              n/a | true   |       0 |    n/a |
| storm-edit-16n      |    1 |            0 |              n/a |             87.0 | n/a    |       0 |      0 |
| storm-edit-8        |    1 |            0 |              n/a |             90.5 | n/a    |       0 |      0 |
| storm-idle-read-16n |    1 |            0 |              n/a |              n/a | n/a    |       0 |      0 |
| storm-idle-read-8   |    1 |            0 |              n/a |              n/a | n/a    |       0 |      0 |
| storm-read-16n      |    1 |            0 |              n/a |             88.7 | n/a    |       0 |      0 |
| storm-read-8        |    1 |            0 |              n/a |             90.6 | n/a    |       0 |      0 |
| storm-thumb-16n     |    1 |            0 |              n/a |             87.9 | n/a    |       0 |      0 |
| storm-thumb-8       |    1 |            0 |              n/a |             90.2 | n/a    |       0 |      0 |
| upload              |    1 |           60 |             23.6 |              n/a | true   |       0 |    n/a |

## Idle access latency

Requests served with the server at rest after the scan: the serving floor. Compare p50 across builds; a p95 or max move without a p50 move points at tail behaviour rather than steady state.

### 50 images

| profile | access      | p50 ms ± CI |  p95 |  max |
| :------ | :---------- | :---------- | ---: | ---: |
| control | prefetch    | 6.9 ±0.1    |  7.0 |  7.0 |
| control | thumb       | 7.8 ±0.1    |  7.9 |  7.9 |
| control | edit        | 8.6 ±0.9    | 10.6 | 11.0 |
| control | scroll_bar  | 0.9 ±0.2    |  1.1 |  1.1 |
| control | scroll_rows | 0.8 ±0.0    |  1.0 |  1.1 |
| control | scroll_data | 1.1 ±0.2    |  1.4 |  1.4 |
| upload  | prefetch    | 11.4 ±0.0   | 11.5 | 11.5 |
| upload  | thumb       | 13.6 ±0.3   | 13.6 | 13.6 |
| upload  | edit        | 14.6 ±0.5   | 15.0 | 15.0 |
| upload  | scroll_bar  | 1.3 ±n/a    |  1.3 |  1.3 |
| upload  | scroll_rows | 1.4 ±0.1    |  1.5 |  1.5 |
| upload  | scroll_data | 2.1 ±0.5    |  2.4 |  2.4 |

### 100 images

| profile | access      | p50 ms ± CI |  p95 |  max |
| :------ | :---------- | :---------- | ---: | ---: |
| control | prefetch    | 10.8 ±0.0   | 10.8 | 10.8 |
| control | thumb       | 11.7 ±0.4   | 12.5 | 12.5 |
| control | edit        | 13.1 ±0.2   | 13.3 | 13.3 |
| control | scroll_bar  | 1.3 ±n/a    |  1.3 |  1.3 |
| control | scroll_rows | 1.5 ±0.0    |  1.5 |  1.5 |
| control | scroll_data | 2.5 ±0.8    |  2.9 |  3.0 |
| upload  | prefetch    | 10.9 ±0.4   | 11.3 | 11.4 |
| upload  | thumb       | 12.6 ±0.1   | 12.7 | 12.7 |
| upload  | edit        | 12.7 ±0.2   | 13.0 | 13.0 |
| upload  | scroll_bar  | 1.4 ±n/a    |  1.4 |  1.4 |
| upload  | scroll_rows | 1.5 ±0.1    |  1.5 |  1.5 |
| upload  | scroll_data | 2.4 ±0.8    |  2.9 |  2.9 |

### 200 images

| profile | access      | p50 ms ± CI |  p95 |  max |
| :------ | :---------- | :---------- | ---: | ---: |
| control | prefetch    | 11.5 ±0.1   | 11.6 | 11.6 |
| control | thumb       | 13.5 ±0.4   | 13.5 | 13.5 |
| control | edit        | 13.4 ±0.2   | 13.7 | 13.7 |
| control | scroll_bar  | 1.3 ±n/a    |  1.3 |  1.3 |
| control | scroll_rows | 1.5 ±0.1    |  1.5 |  1.5 |
| control | scroll_data | 3.0 ±1.4    |  4.2 |  4.3 |
| upload  | prefetch    | 11.1 ±0.2   | 11.1 | 11.1 |
| upload  | thumb       | 12.3 ±0.4   | 12.7 | 12.7 |
| upload  | edit        | 13.0 ±0.1   | 13.0 | 13.0 |
| upload  | scroll_bar  | 1.4 ±n/a    |  1.4 |  1.4 |
| upload  | scroll_rows | 1.5 ±0.1    |  1.5 |  1.5 |
| upload  | scroll_data | 3.4 ±1.5    |  4.3 |  4.3 |

## During-scan access latency

The same accesses while an album scan runs. The gap to idle is the indexing-contention cost and the primary regression signal for the request path.

### 50 images

| profile | access      | p50 ms ± CI |  p95 |  max |
| :------ | :---------- | :---------- | ---: | ---: |
| control | prefetch    | 9.0 ±0.8    | 20.6 | 25.7 |
| control | thumb       | 1.9 ±0.2    | 19.3 | 65.9 |
| control | edit        | 10.2 ±0.6   | 18.4 | 41.0 |
| control | scroll_bar  | 0.7 ±0.1    |  1.6 |  1.6 |
| control | scroll_rows | 0.8 ±0.0    |  2.1 |  6.1 |
| control | scroll_data | 1.2 ±0.2    |  5.0 | 12.0 |
| upload  | prefetch    | 9.0 ±0.9    | 16.5 | 19.0 |
| upload  | thumb       | 1.8 ±0.2    |  7.7 | 13.6 |
| upload  | edit        | 10.0 ±0.3   | 15.2 | 21.4 |
| upload  | scroll_bar  | 0.7 ±0.1    |  1.5 |  1.6 |
| upload  | scroll_rows | 0.8 ±0.1    |  1.8 |  4.0 |
| upload  | scroll_data | 1.7 ±0.1    |  4.9 |  9.5 |

### 100 images

| profile | access      | p50 ms ± CI |  p95 |  max |
| :------ | :---------- | :---------- | ---: | ---: |
| control | prefetch    | 8.5 ±0.5    | 15.0 | 19.0 |
| control | thumb       | 1.2 ±0.1    |  8.3 | 16.8 |
| control | edit        | 9.1 ±0.2    | 10.9 | 14.4 |
| control | scroll_bar  | 0.7 ±0.0    |  1.0 |  1.3 |
| control | scroll_rows | 0.7 ±0.0    |  0.8 |  4.0 |
| control | scroll_data | 0.9 ±0.0    |  3.3 |  7.5 |
| upload  | prefetch    | 8.7 ±0.5    | 16.5 | 18.3 |
| upload  | thumb       | 1.5 ±0.2    | 11.9 | 36.0 |
| upload  | edit        | 9.4 ±0.4    | 12.1 | 18.9 |
| upload  | scroll_bar  | 0.7 ±0.0    |  1.6 |  1.6 |
| upload  | scroll_rows | 0.7 ±0.0    |  1.9 |  4.4 |
| upload  | scroll_data | 2.1 ±0.2    |  5.4 |  9.1 |

### 200 images

| profile | access      | p50 ms ± CI |  p95 |  max |
| :------ | :---------- | :---------- | ---: | ---: |
| control | prefetch    | 9.8 ±1.2    | 21.9 | 31.7 |
| control | thumb       | 2.0 ±0.4    | 17.0 | 43.1 |
| control | edit        | 9.8 ±0.2    | 13.9 | 15.8 |
| control | scroll_bar  | 0.7 ±0.0    |  1.6 |  3.5 |
| control | scroll_rows | 0.8 ±0.0    |  3.0 |  4.4 |
| control | scroll_data | 1.2 ±0.3    |  7.2 | 16.1 |
| upload  | prefetch    | 10.1 ±1.7   | 23.2 | 36.7 |
| upload  | thumb       | 1.8 ±0.1    | 20.2 | 36.9 |
| upload  | edit        | 9.8 ±0.2    | 15.0 | 19.0 |
| upload  | scroll_bar  | 0.8 ±0.1    |  1.6 |  1.7 |
| upload  | scroll_rows | 0.7 ±0.1    |  3.9 |  4.0 |
| upload  | scroll_data | 3.1 ±0.3    |  8.0 | 14.0 |

## Concurrent load

Sustained load at the client count named by the storm profile, during a scan or against the tree at rest. p50 is loaded serving latency; p95 and max expose queueing tails. Only the profile's storm access is shown, so the rows are comparable across client counts.

### 50 images

| profile             | background | access   | p50 ms ± CI |   p95 |   max |
| :------------------ | :--------- | :------- | :---------- | ----: | ----: |
| storm-edit-16n      | scan       | edit     | 180.4 ±13.3 | 414.1 | 855.5 |
| storm-edit-8        | scan       | edit     | 10.6 ±0.2   |  22.3 |  78.5 |
| storm-idle-read-16n | idle       | prefetch | 1.9 ±0.1    |   7.6 |  35.9 |
| storm-idle-read-8   | idle       | prefetch | 2.1 ±0.2    |   7.2 |  10.4 |
| storm-read-16n      | scan       | prefetch | 3.3 ±0.1    |  16.4 |  52.7 |
| storm-read-8        | scan       | prefetch | 3.3 ±0.1    |  11.7 |  45.1 |
| storm-thumb-16n     | scan       | thumb    | 7.5 ±0.2    |  30.9 |  85.5 |
| storm-thumb-8       | scan       | thumb    | 7.4 ±0.2    |  19.7 |  91.2 |

### 100 images

| profile             | background | access   | p50 ms ± CI |   p95 |    max |
| :------------------ | :--------- | :------- | :---------- | ----: | -----: |
| storm-edit-16n      | scan       | edit     | 205.8 ±0.8  | 428.9 | 1268.2 |
| storm-edit-8        | scan       | edit     | 11.4 ±0.1   |  22.7 |  101.4 |
| storm-idle-read-16n | idle       | prefetch | 3.2 ±0.1    |  10.6 |   59.8 |
| storm-idle-read-8   | idle       | prefetch | 2.1 ±0.2    |   3.3 |    4.5 |
| storm-read-16n      | scan       | prefetch | 3.3 ±0.1    |  66.8 |  385.0 |
| storm-read-8        | scan       | prefetch | 3.2 ±0.1    |  13.4 |   55.1 |
| storm-thumb-16n     | scan       | thumb    | 13.9 ±0.4   |  90.6 |  252.3 |
| storm-thumb-8       | scan       | thumb    | 8.3 ±0.1    |  19.8 |   89.2 |

### 200 images

| profile             | background | access   | p50 ms ± CI |   p95 |    max |
| :------------------ | :--------- | :------- | :---------- | ----: | -----: |
| storm-edit-16n      | scan       | edit     | 204.8 ±0.4  | 417.4 | 1407.6 |
| storm-edit-8        | scan       | edit     | 12.6 ±0.1   |  25.9 |   88.3 |
| storm-idle-read-16n | idle       | prefetch | 1.8 ±0.0    | 199.9 |  315.4 |
| storm-idle-read-8   | idle       | prefetch | 2.2 ±0.3    |   4.5 |    5.7 |
| storm-read-16n      | scan       | prefetch | 4.8 ±0.1    | 241.3 |  507.5 |
| storm-read-8        | scan       | prefetch | 3.2 ±0.0    |  16.9 |   70.9 |
| storm-thumb-16n     | scan       | thumb    | 13.5 ±0.2   |  63.9 |  230.8 |
| storm-thumb-8       | scan       | thumb    | 8.2 ±0.1    |  16.8 |   63.7 |

## Scan wall and stages

How long populate scans took and where the time went, from the observer drain table. Wall well above the sum of the stage rows points at work outside the instrumented stages or at permit waits.

### 50 images

| profile             | p50 s ± CI | p95 | max |
| :------------------ | :--------- | --: | --: |
| control             | 5.3 ±0.4   | 5.7 | 5.7 |
| storm-edit-16n      | 6.1 ±n/a   | 6.1 | 6.1 |
| storm-edit-8        | 5.3 ±n/a   | 5.3 | 5.3 |
| storm-idle-read-16n | 5.5 ±n/a   | 5.5 | 5.5 |
| storm-idle-read-8   | 5.4 ±n/a   | 5.4 | 5.4 |
| storm-read-16n      | 5.9 ±n/a   | 5.9 | 5.9 |
| storm-read-8        | 6.2 ±n/a   | 6.2 | 6.2 |
| storm-thumb-16n     | 5.9 ±n/a   | 5.9 | 5.9 |
| storm-thumb-8       | 6.0 ±n/a   | 6.0 | 6.0 |
| upload              | 6.6 ±0.1   | 6.7 | 6.7 |

| profile             | stage                   | p50 us ± CI    |    p95 |    max |
| :------------------ | :---------------------- | :------------- | -----: | -----: |
| control             | blocking_queue_wait     | 37.5 ±31.5     |   78.8 |   85.0 |
| control             | compute_locate          | 0.0 ±0.0       |    0.0 |    0.0 |
| control             | cpu_exec                | 322.0 ±10.5    |  335.2 |  337.0 |
| control             | cpu_permit_wait         | 2556.0 ±783.0  | 3405.5 | 3446.0 |
| control             | filter_par_iter         | 1513.5 ±1512.0 | 3672.2 | 4029.0 |
| control             | filter_snapshot_clone   | 550.0 ±198.0   |  730.4 |  731.0 |
| control             | prefetch_total          | 27.5 ±2.5      |   29.7 |   30.0 |
| control             | tree_refresh_build      | 457.0 ±167.5   |  622.2 |  629.0 |
| control             | tree_refresh_sort       | 12.5 ±5.5      |   18.0 |   18.0 |
| control             | tree_refresh_total      | 548.0 ±230.5   |  783.6 |  794.0 |
| control             | tree_refresh_write_wait | 0.0 ±0.0       |    0.0 |    0.0 |
| storm-edit-16n      | blocking_queue_wait     | 24.0 ±n/a      |   24.0 |   24.0 |
| storm-edit-16n      | compute_locate          | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-edit-16n      | cpu_exec                | 673.0 ±n/a     |  673.0 |  673.0 |
| storm-edit-16n      | cpu_permit_wait         | 1659.0 ±n/a    | 1659.0 | 1659.0 |
| storm-edit-16n      | filter_par_iter         | 509.0 ±n/a     |  509.0 |  509.0 |
| storm-edit-16n      | filter_snapshot_clone   | 202.0 ±n/a     |  202.0 |  202.0 |
| storm-edit-16n      | prefetch_total          | 418.0 ±n/a     |  418.0 |  418.0 |
| storm-edit-16n      | tree_refresh_build      | 189.0 ±n/a     |  189.0 |  189.0 |
| storm-edit-16n      | tree_refresh_sort       | 1.0 ±n/a       |    1.0 |    1.0 |
| storm-edit-16n      | tree_refresh_total      | 200.0 ±n/a     |  200.0 |  200.0 |
| storm-edit-16n      | tree_refresh_write_wait | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-edit-8        | blocking_queue_wait     | 26.0 ±n/a      |   26.0 |   26.0 |
| storm-edit-8        | compute_locate          | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-edit-8        | cpu_exec                | 373.0 ±n/a     |  373.0 |  373.0 |
| storm-edit-8        | cpu_permit_wait         | 1379.0 ±n/a    | 1379.0 | 1379.0 |
| storm-edit-8        | filter_par_iter         | 445.0 ±n/a     |  445.0 |  445.0 |
| storm-edit-8        | filter_snapshot_clone   | 38.0 ±n/a      |   38.0 |   38.0 |
| storm-edit-8        | prefetch_total          | 35.0 ±n/a      |   35.0 |   35.0 |
| storm-edit-8        | tree_refresh_build      | 152.0 ±n/a     |  152.0 |  152.0 |
| storm-edit-8        | tree_refresh_sort       | 1.0 ±n/a       |    1.0 |    1.0 |
| storm-edit-8        | tree_refresh_total      | 157.0 ±n/a     |  157.0 |  157.0 |
| storm-edit-8        | tree_refresh_write_wait | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-idle-read-16n | cpu_exec                | 333.0 ±n/a     |  333.0 |  333.0 |
| storm-idle-read-16n | cpu_permit_wait         | 2468.0 ±n/a    | 2468.0 | 2468.0 |
| storm-idle-read-8   | cpu_exec                | 336.0 ±n/a     |  336.0 |  336.0 |
| storm-idle-read-8   | cpu_permit_wait         | 1585.0 ±n/a    | 1585.0 | 1585.0 |
| storm-read-16n      | blocking_queue_wait     | 90.0 ±n/a      |   90.0 |   90.0 |
| storm-read-16n      | compute_locate          | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-read-16n      | cpu_exec                | 330.0 ±n/a     |  330.0 |  330.0 |
| storm-read-16n      | cpu_permit_wait         | 4269.0 ±n/a    | 4269.0 | 4269.0 |
| storm-read-16n      | filter_par_iter         | 5437.0 ±n/a    | 5437.0 | 5437.0 |
| storm-read-16n      | filter_snapshot_clone   | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-read-16n      | prefetch_total          | 13.0 ±n/a      |   13.0 |   13.0 |
| storm-read-16n      | tree_refresh_build      | 404.0 ±n/a     |  404.0 |  404.0 |
| storm-read-16n      | tree_refresh_sort       | 11.0 ±n/a      |   11.0 |   11.0 |
| storm-read-16n      | tree_refresh_total      | 455.0 ±n/a     |  455.0 |  455.0 |
| storm-read-16n      | tree_refresh_write_wait | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-read-8        | blocking_queue_wait     | 108.0 ±n/a     |  108.0 |  108.0 |
| storm-read-8        | compute_locate          | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-read-8        | cpu_exec                | 380.0 ±n/a     |  380.0 |  380.0 |
| storm-read-8        | cpu_permit_wait         | 4029.0 ±n/a    | 4029.0 | 4029.0 |
| storm-read-8        | filter_par_iter         | 4037.0 ±n/a    | 4037.0 | 4037.0 |
| storm-read-8        | filter_snapshot_clone   | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-read-8        | prefetch_total          | 15.0 ±n/a      |   15.0 |   15.0 |
| storm-read-8        | tree_refresh_build      | 320.0 ±n/a     |  320.0 |  320.0 |
| storm-read-8        | tree_refresh_sort       | 8.0 ±n/a       |    8.0 |    8.0 |
| storm-read-8        | tree_refresh_total      | 371.0 ±n/a     |  371.0 |  371.0 |
| storm-read-8        | tree_refresh_write_wait | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-thumb-16n     | blocking_queue_wait     | 23.0 ±n/a      |   23.0 |   23.0 |
| storm-thumb-16n     | compute_locate          | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-thumb-16n     | cpu_exec                | 358.0 ±n/a     |  358.0 |  358.0 |
| storm-thumb-16n     | cpu_permit_wait         | 2665.0 ±n/a    | 2665.0 | 2665.0 |
| storm-thumb-16n     | filter_par_iter         | 565.0 ±n/a     |  565.0 |  565.0 |
| storm-thumb-16n     | filter_snapshot_clone   | 172.0 ±n/a     |  172.0 |  172.0 |
| storm-thumb-16n     | prefetch_total          | 575.0 ±n/a     |  575.0 |  575.0 |
| storm-thumb-16n     | tree_refresh_build      | 163.0 ±n/a     |  163.0 |  163.0 |
| storm-thumb-16n     | tree_refresh_sort       | 1.0 ±n/a       |    1.0 |    1.0 |
| storm-thumb-16n     | tree_refresh_total      | 170.0 ±n/a     |  170.0 |  170.0 |
| storm-thumb-16n     | tree_refresh_write_wait | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-thumb-8       | blocking_queue_wait     | 367.0 ±n/a     |  367.0 |  367.0 |
| storm-thumb-8       | compute_locate          | 0.0 ±n/a       |    0.0 |    0.0 |
| storm-thumb-8       | cpu_exec                | 344.0 ±n/a     |  344.0 |  344.0 |
| storm-thumb-8       | cpu_permit_wait         | 3204.0 ±n/a    | 3204.0 | 3204.0 |
| storm-thumb-8       | filter_par_iter         | 399.0 ±n/a     |  399.0 |  399.0 |
| storm-thumb-8       | filter_snapshot_clone   | 38.0 ±n/a      |   38.0 |   38.0 |
| storm-thumb-8       | prefetch_total          | 429.0 ±n/a     |  429.0 |  429.0 |
| storm-thumb-8       | tree_refresh_build      | 109.0 ±n/a     |  109.0 |  109.0 |
| storm-thumb-8       | tree_refresh_sort       | 1.0 ±n/a       |    1.0 |    1.0 |
| storm-thumb-8       | tree_refresh_total      | 113.0 ±n/a     |  113.0 |  113.0 |
| storm-thumb-8       | tree_refresh_write_wait | 0.0 ±n/a       |    0.0 |    0.0 |
| upload              | blocking_queue_wait     | 33.5 ±1.5      |   34.9 |   35.0 |
| upload              | compute_locate          | 0.0 ±0.0       |    0.0 |    0.0 |
| upload              | cpu_exec                | 303.5 ±1.5     |  304.9 |  305.0 |
| upload              | cpu_permit_wait         | 1932.5 ±4.5    | 1936.5 | 1937.0 |
| upload              | filter_par_iter         | 2374.0 ±19.0   | 2391.1 | 2393.0 |
| upload              | filter_snapshot_clone   | 961.5 ±4.5     |  965.5 |  966.0 |
| upload              | prefetch_total          | 37.5 ±13.5     |   49.6 |   51.0 |
| upload              | tree_refresh_build      | 748.0 ±5.0     |  752.5 |  753.0 |
| upload              | tree_refresh_sort       | 27.0 ±1.0      |   27.9 |   28.0 |
| upload              | tree_refresh_total      | 960.5 ±6.5     |  966.4 |  967.0 |
| upload              | tree_refresh_write_wait | 0.0 ±0.0       |    0.0 |    0.0 |

### 100 images

| profile             | p50 s ± CI |  p95 |  max |
| :------------------ | :--------- | ---: | ---: |
| control             | 9.4 ±0.3   |  9.7 |  9.7 |
| storm-edit-16n      | 10.5 ±n/a  | 10.5 | 10.5 |
| storm-edit-8        | 9.6 ±n/a   |  9.6 |  9.6 |
| storm-idle-read-16n | 9.9 ±n/a   |  9.9 |  9.9 |
| storm-idle-read-8   | 9.7 ±n/a   |  9.7 |  9.7 |
| storm-read-16n      | 10.8 ±n/a  | 10.8 | 10.8 |
| storm-read-8        | 10.2 ±n/a  | 10.2 | 10.2 |
| storm-thumb-16n     | 10.6 ±n/a  | 10.6 | 10.6 |
| storm-thumb-8       | 10.5 ±n/a  | 10.5 | 10.5 |
| upload              | 10.9 ±0.0  | 10.9 | 10.9 |

| profile             | stage                   | p50 us ± CI   |     p95 |     max |
| :------------------ | :---------------------- | :------------ | ------: | ------: |
| control             | blocking_queue_wait     | 22.5 ±1.5     |    23.9 |    24.0 |
| control             | compute_locate          | 0.0 ±0.0      |     0.0 |     0.0 |
| control             | cpu_exec                | 275.0 ±0.0    |   275.0 |   275.0 |
| control             | cpu_permit_wait         | 2435.0 ±61.0  |  2489.9 |  2496.0 |
| control             | filter_par_iter         | 2520.0 ±767.0 |  3210.3 |  3287.0 |
| control             | filter_snapshot_clone   | 947.5 ±340.5  |  1253.9 |  1288.0 |
| control             | prefetch_total          | 25.0 ±2.0     |    26.8 |    27.0 |
| control             | tree_refresh_build      | 779.5 ±218.5  |   976.1 |   998.0 |
| control             | tree_refresh_sort       | 23.0 ±10.0    |    32.0 |    33.0 |
| control             | tree_refresh_total      | 947.5 ±342.5  |  1255.8 |  1290.0 |
| control             | tree_refresh_write_wait | 0.0 ±0.0      |     0.0 |     0.0 |
| storm-edit-16n      | blocking_queue_wait     | 21.0 ±n/a     |    21.0 |    21.0 |
| storm-edit-16n      | compute_locate          | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-edit-16n      | cpu_exec                | 527.0 ±n/a    |   527.0 |   527.0 |
| storm-edit-16n      | cpu_permit_wait         | 2356.0 ±n/a   |  2356.0 |  2356.0 |
| storm-edit-16n      | filter_par_iter         | 538.0 ±n/a    |   538.0 |   538.0 |
| storm-edit-16n      | filter_snapshot_clone   | 202.0 ±n/a    |   202.0 |   202.0 |
| storm-edit-16n      | prefetch_total          | 476.0 ±n/a    |   476.0 |   476.0 |
| storm-edit-16n      | tree_refresh_build      | 183.0 ±n/a    |   183.0 |   183.0 |
| storm-edit-16n      | tree_refresh_sort       | 1.0 ±n/a      |     1.0 |     1.0 |
| storm-edit-16n      | tree_refresh_total      | 200.0 ±n/a    |   200.0 |   200.0 |
| storm-edit-16n      | tree_refresh_write_wait | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-edit-8        | blocking_queue_wait     | 114.0 ±n/a    |   114.0 |   114.0 |
| storm-edit-8        | compute_locate          | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-edit-8        | cpu_exec                | 293.0 ±n/a    |   293.0 |   293.0 |
| storm-edit-8        | cpu_permit_wait         | 3011.0 ±n/a   |  3011.0 |  3011.0 |
| storm-edit-8        | filter_par_iter         | 349.0 ±n/a    |   349.0 |   349.0 |
| storm-edit-8        | filter_snapshot_clone   | 195.0 ±n/a    |   195.0 |   195.0 |
| storm-edit-8        | prefetch_total          | 646.0 ±n/a    |   646.0 |   646.0 |
| storm-edit-8        | tree_refresh_build      | 186.0 ±n/a    |   186.0 |   186.0 |
| storm-edit-8        | tree_refresh_sort       | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-edit-8        | tree_refresh_total      | 193.0 ±n/a    |   193.0 |   193.0 |
| storm-edit-8        | tree_refresh_write_wait | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-idle-read-16n | cpu_exec                | 277.0 ±n/a    |   277.0 |   277.0 |
| storm-idle-read-16n | cpu_permit_wait         | 2575.0 ±n/a   |  2575.0 |  2575.0 |
| storm-idle-read-8   | cpu_exec                | 267.0 ±n/a    |   267.0 |   267.0 |
| storm-idle-read-8   | cpu_permit_wait         | 1984.0 ±n/a   |  1984.0 |  1984.0 |
| storm-read-16n      | blocking_queue_wait     | 146.0 ±n/a    |   146.0 |   146.0 |
| storm-read-16n      | compute_locate          | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-read-16n      | cpu_exec                | 278.0 ±n/a    |   278.0 |   278.0 |
| storm-read-16n      | cpu_permit_wait         | 8965.0 ±n/a   |  8965.0 |  8965.0 |
| storm-read-16n      | filter_par_iter         | 65704.0 ±n/a  | 65704.0 | 65704.0 |
| storm-read-16n      | filter_snapshot_clone   | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-read-16n      | prefetch_total          | 14.0 ±n/a     |    14.0 |    14.0 |
| storm-read-16n      | tree_refresh_build      | 737.0 ±n/a    |   737.0 |   737.0 |
| storm-read-16n      | tree_refresh_sort       | 19.0 ±n/a     |    19.0 |    19.0 |
| storm-read-16n      | tree_refresh_total      | 804.0 ±n/a    |   804.0 |   804.0 |
| storm-read-16n      | tree_refresh_write_wait | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-read-8        | blocking_queue_wait     | 266.0 ±n/a    |   266.0 |   266.0 |
| storm-read-8        | compute_locate          | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-read-8        | cpu_exec                | 290.0 ±n/a    |   290.0 |   290.0 |
| storm-read-8        | cpu_permit_wait         | 4589.0 ±n/a   |  4589.0 |  4589.0 |
| storm-read-8        | filter_par_iter         | 10234.0 ±n/a  | 10234.0 | 10234.0 |
| storm-read-8        | filter_snapshot_clone   | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-read-8        | prefetch_total          | 15.0 ±n/a     |    15.0 |    15.0 |
| storm-read-8        | tree_refresh_build      | 644.0 ±n/a    |   644.0 |   644.0 |
| storm-read-8        | tree_refresh_sort       | 19.0 ±n/a     |    19.0 |    19.0 |
| storm-read-8        | tree_refresh_total      | 764.0 ±n/a    |   764.0 |   764.0 |
| storm-read-8        | tree_refresh_write_wait | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-thumb-16n     | blocking_queue_wait     | 22.0 ±n/a     |    22.0 |    22.0 |
| storm-thumb-16n     | compute_locate          | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-thumb-16n     | cpu_exec                | 284.0 ±n/a    |   284.0 |   284.0 |
| storm-thumb-16n     | cpu_permit_wait         | 5606.0 ±n/a   |  5606.0 |  5606.0 |
| storm-thumb-16n     | filter_par_iter         | 572.0 ±n/a    |   572.0 |   572.0 |
| storm-thumb-16n     | filter_snapshot_clone   | 121.0 ±n/a    |   121.0 |   121.0 |
| storm-thumb-16n     | prefetch_total          | 376.0 ±n/a    |   376.0 |   376.0 |
| storm-thumb-16n     | tree_refresh_build      | 149.0 ±n/a    |   149.0 |   149.0 |
| storm-thumb-16n     | tree_refresh_sort       | 1.0 ±n/a      |     1.0 |     1.0 |
| storm-thumb-16n     | tree_refresh_total      | 161.0 ±n/a    |   161.0 |   161.0 |
| storm-thumb-16n     | tree_refresh_write_wait | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-thumb-8       | blocking_queue_wait     | 135.0 ±n/a    |   135.0 |   135.0 |
| storm-thumb-8       | compute_locate          | 0.0 ±n/a      |     0.0 |     0.0 |
| storm-thumb-8       | cpu_exec                | 279.0 ±n/a    |   279.0 |   279.0 |
| storm-thumb-8       | cpu_permit_wait         | 10884.0 ±n/a  | 10884.0 | 10884.0 |
| storm-thumb-8       | filter_par_iter         | 598.0 ±n/a    |   598.0 |   598.0 |
| storm-thumb-8       | filter_snapshot_clone   | 78.0 ±n/a     |    78.0 |    78.0 |
| storm-thumb-8       | prefetch_total          | 579.0 ±n/a    |   579.0 |   579.0 |
| storm-thumb-8       | tree_refresh_build      | 72.0 ±n/a     |    72.0 |    72.0 |
| storm-thumb-8       | tree_refresh_sort       | 1.0 ±n/a      |     1.0 |     1.0 |
| storm-thumb-8       | tree_refresh_total      | 76.0 ±n/a     |    76.0 |    76.0 |
| storm-thumb-8       | tree_refresh_write_wait | 0.0 ±n/a      |     0.0 |     0.0 |
| upload              | blocking_queue_wait     | 25.0 ±2.0     |    26.8 |    27.0 |
| upload              | compute_locate          | 0.0 ±0.0      |     0.0 |     0.0 |
| upload              | cpu_exec                | 262.5 ±2.5    |   264.8 |   265.0 |
| upload              | cpu_permit_wait         | 2595.5 ±105.5 |  2690.4 |  2701.0 |
| upload              | filter_par_iter         | 3962.0 ±5.0   |  3966.5 |  3967.0 |
| upload              | filter_snapshot_clone   | 1526.0 ±1.0   |  1526.9 |  1527.0 |
| upload              | prefetch_total          | 27.5 ±1.5     |    28.9 |    29.0 |
| upload              | tree_refresh_build      | 1186.0 ±3.0   |  1188.7 |  1189.0 |
| upload              | tree_refresh_sort       | 32.0 ±4.0     |    35.6 |    36.0 |
| upload              | tree_refresh_total      | 1523.5 ±1.5   |  1524.8 |  1525.0 |
| upload              | tree_refresh_write_wait | 0.0 ±0.0      |     0.0 |     0.0 |

### 200 images

| profile             | p50 s ± CI |  p95 |  max |
| :------------------ | :--------- | ---: | ---: |
| control             | 18.2 ±0.0  | 18.2 | 18.2 |
| storm-edit-16n      | 19.7 ±n/a  | 19.7 | 19.7 |
| storm-edit-8        | 19.7 ±n/a  | 19.7 | 19.7 |
| storm-idle-read-16n | 18.4 ±n/a  | 18.4 | 18.4 |
| storm-idle-read-8   | 18.4 ±n/a  | 18.4 | 18.4 |
| storm-read-16n      | 22.0 ±n/a  | 22.0 | 22.0 |
| storm-read-8        | 19.7 ±n/a  | 19.7 | 19.7 |
| storm-thumb-16n     | 20.6 ±n/a  | 20.6 | 20.6 |
| storm-thumb-8       | 19.3 ±n/a  | 19.3 | 19.3 |
| upload              | 19.6 ±0.2  | 19.8 | 19.8 |

| profile             | stage                   | p50 us ± CI      |      p95 |      max |
| :------------------ | :---------------------- | :--------------- | -------: | -------: |
| control             | blocking_queue_wait     | 69.0 ±48.0       |    112.2 |    117.0 |
| control             | compute_locate          | 0.0 ±0.0         |      0.0 |      0.0 |
| control             | cpu_exec                | 252.0 ±7.0       |    258.3 |    259.0 |
| control             | cpu_permit_wait         | 13690.0 ±11632.0 |  24158.8 |  25322.0 |
| control             | filter_par_iter         | 4719.0 ±1705.0   |   6253.5 |   6424.0 |
| control             | filter_snapshot_clone   | 1884.0 ±576.0    |   2402.4 |   2460.0 |
| control             | prefetch_total          | 28.0 ±3.0        |     30.7 |     31.0 |
| control             | tree_refresh_build      | 1506.5 ±377.5    |   1846.2 |   1884.0 |
| control             | tree_refresh_sort       | 49.5 ±18.5       |     66.2 |     68.0 |
| control             | tree_refresh_total      | 1898.5 ±559.5    |   2402.1 |   2458.0 |
| control             | tree_refresh_write_wait | 0.0 ±0.0         |      0.0 |      0.0 |
| storm-edit-16n      | blocking_queue_wait     | 25.0 ±n/a        |     25.0 |     25.0 |
| storm-edit-16n      | compute_locate          | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-edit-16n      | cpu_exec                | 486.0 ±n/a       |    486.0 |    486.0 |
| storm-edit-16n      | cpu_permit_wait         | 5037.0 ±n/a      |   5037.0 |   5037.0 |
| storm-edit-16n      | filter_par_iter         | 340.0 ±n/a       |    340.0 |    340.0 |
| storm-edit-16n      | filter_snapshot_clone   | 127.0 ±n/a       |    127.0 |    127.0 |
| storm-edit-16n      | prefetch_total          | 209.0 ±n/a       |    209.0 |    209.0 |
| storm-edit-16n      | tree_refresh_build      | 143.0 ±n/a       |    143.0 |    143.0 |
| storm-edit-16n      | tree_refresh_sort       | 1.0 ±n/a         |      1.0 |      1.0 |
| storm-edit-16n      | tree_refresh_total      | 156.0 ±n/a       |    156.0 |    156.0 |
| storm-edit-16n      | tree_refresh_write_wait | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-edit-8        | blocking_queue_wait     | 56.0 ±n/a        |     56.0 |     56.0 |
| storm-edit-8        | compute_locate          | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-edit-8        | cpu_exec                | 242.0 ±n/a       |    242.0 |    242.0 |
| storm-edit-8        | cpu_permit_wait         | 3703.0 ±n/a      |   3703.0 |   3703.0 |
| storm-edit-8        | filter_par_iter         | 381.0 ±n/a       |    381.0 |    381.0 |
| storm-edit-8        | filter_snapshot_clone   | 89.0 ±n/a        |     89.0 |     89.0 |
| storm-edit-8        | prefetch_total          | 409.0 ±n/a       |    409.0 |    409.0 |
| storm-edit-8        | tree_refresh_build      | 83.0 ±n/a        |     83.0 |     83.0 |
| storm-edit-8        | tree_refresh_sort       | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-edit-8        | tree_refresh_total      | 87.0 ±n/a        |     87.0 |     87.0 |
| storm-edit-8        | tree_refresh_write_wait | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-idle-read-16n | cpu_exec                | 224.0 ±n/a       |    224.0 |    224.0 |
| storm-idle-read-16n | cpu_permit_wait         | 1933.0 ±n/a      |   1933.0 |   1933.0 |
| storm-idle-read-8   | cpu_exec                | 231.0 ±n/a       |    231.0 |    231.0 |
| storm-idle-read-8   | cpu_permit_wait         | 812.0 ±n/a       |    812.0 |    812.0 |
| storm-read-16n      | blocking_queue_wait     | 277.0 ±n/a       |    277.0 |    277.0 |
| storm-read-16n      | compute_locate          | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-read-16n      | cpu_exec                | 303.0 ±n/a       |    303.0 |    303.0 |
| storm-read-16n      | cpu_permit_wait         | 15247.0 ±n/a     |  15247.0 |  15247.0 |
| storm-read-16n      | filter_par_iter         | 254144.0 ±n/a    | 254144.0 | 254144.0 |
| storm-read-16n      | filter_snapshot_clone   | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-read-16n      | prefetch_total          | 13.0 ±n/a        |     13.0 |     13.0 |
| storm-read-16n      | tree_refresh_build      | 1175.0 ±n/a      |   1175.0 |   1175.0 |
| storm-read-16n      | tree_refresh_sort       | 35.0 ±n/a        |     35.0 |     35.0 |
| storm-read-16n      | tree_refresh_total      | 1342.0 ±n/a      |   1342.0 |   1342.0 |
| storm-read-16n      | tree_refresh_write_wait | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-read-8        | blocking_queue_wait     | 189.0 ±n/a       |    189.0 |    189.0 |
| storm-read-8        | compute_locate          | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-read-8        | cpu_exec                | 331.0 ±n/a       |    331.0 |    331.0 |
| storm-read-8        | cpu_permit_wait         | 7200.0 ±n/a      |   7200.0 |   7200.0 |
| storm-read-8        | filter_par_iter         | 26994.0 ±n/a     |  26994.0 |  26994.0 |
| storm-read-8        | filter_snapshot_clone   | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-read-8        | prefetch_total          | 15.0 ±n/a        |     15.0 |     15.0 |
| storm-read-8        | tree_refresh_build      | 1063.0 ±n/a      |   1063.0 |   1063.0 |
| storm-read-8        | tree_refresh_sort       | 30.0 ±n/a        |     30.0 |     30.0 |
| storm-read-8        | tree_refresh_total      | 1275.0 ±n/a      |   1275.0 |   1275.0 |
| storm-read-8        | tree_refresh_write_wait | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-thumb-16n     | blocking_queue_wait     | 21.0 ±n/a        |     21.0 |     21.0 |
| storm-thumb-16n     | compute_locate          | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-thumb-16n     | cpu_exec                | 269.0 ±n/a       |    269.0 |    269.0 |
| storm-thumb-16n     | cpu_permit_wait         | 6230.0 ±n/a      |   6230.0 |   6230.0 |
| storm-thumb-16n     | filter_par_iter         | 754.0 ±n/a       |    754.0 |    754.0 |
| storm-thumb-16n     | filter_snapshot_clone   | 196.0 ±n/a       |    196.0 |    196.0 |
| storm-thumb-16n     | prefetch_total          | 854.0 ±n/a       |    854.0 |    854.0 |
| storm-thumb-16n     | tree_refresh_build      | 181.0 ±n/a       |    181.0 |    181.0 |
| storm-thumb-16n     | tree_refresh_sort       | 1.0 ±n/a         |      1.0 |      1.0 |
| storm-thumb-16n     | tree_refresh_total      | 195.0 ±n/a       |    195.0 |    195.0 |
| storm-thumb-16n     | tree_refresh_write_wait | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-thumb-8       | blocking_queue_wait     | 583.0 ±n/a       |    583.0 |    583.0 |
| storm-thumb-8       | compute_locate          | 0.0 ±n/a         |      0.0 |      0.0 |
| storm-thumb-8       | cpu_exec                | 270.0 ±n/a       |    270.0 |    270.0 |
| storm-thumb-8       | cpu_permit_wait         | 4942.0 ±n/a      |   4942.0 |   4942.0 |
| storm-thumb-8       | filter_par_iter         | 1022.0 ±n/a      |   1022.0 |   1022.0 |
| storm-thumb-8       | filter_snapshot_clone   | 129.0 ±n/a       |    129.0 |    129.0 |
| storm-thumb-8       | prefetch_total          | 1028.0 ±n/a      |   1028.0 |   1028.0 |
| storm-thumb-8       | tree_refresh_build      | 123.0 ±n/a       |    123.0 |    123.0 |
| storm-thumb-8       | tree_refresh_sort       | 1.0 ±n/a         |      1.0 |      1.0 |
| storm-thumb-8       | tree_refresh_total      | 127.0 ±n/a       |    127.0 |    127.0 |
| storm-thumb-8       | tree_refresh_write_wait | 0.0 ±n/a         |      0.0 |      0.0 |
| upload              | blocking_queue_wait     | 34.0 ±12.0       |     44.8 |     46.0 |
| upload              | compute_locate          | 0.0 ±0.0         |      0.0 |      0.0 |
| upload              | cpu_exec                | 266.0 ±6.0       |    271.4 |    272.0 |
| upload              | cpu_permit_wait         | 2483.5 ±546.5    |   2975.3 |   3030.0 |
| upload              | filter_par_iter         | 7099.5 ±26.5     |   7123.4 |   7126.0 |
| upload              | filter_snapshot_clone   | 2709.5 ±3.5      |   2712.7 |   2713.0 |
| upload              | prefetch_total          | 29.5 ±2.5        |     31.8 |     32.0 |
| upload              | tree_refresh_build      | 2091.0 ±2.0      |   2092.8 |   2093.0 |
| upload              | tree_refresh_sort       | 58.0 ±0.0        |     58.0 |     58.0 |
| upload              | tree_refresh_total      | 2708.0 ±3.0      |   2710.7 |   2711.0 |
| upload              | tree_refresh_write_wait | 0.0 ±0.0         |      0.0 |      0.0 |

## Upload

Write path end to end: submit-to-accept and time until the uploaded row is served. `visible` includes the indexing drain the upload triggers, so it tracks ingest plus scan progress.

### 50 images

| profile | phase   | p50 ms ± CI |    p95 |    max |
| :------ | :------ | :---------- | -----: | -----: |
| upload  | submit  | 7.0 ±2.2    |   23.2 |   26.0 |
| upload  | visible | 831.5 ±54.0 | 1053.2 | 4762.0 |

### 100 images

| profile | phase   | p50 ms ± CI |    p95 |    max |
| :------ | :------ | :---------- | -----: | -----: |
| upload  | submit  | 9.0 ±1.5    |   16.9 |   34.0 |
| upload  | visible | 848.5 ±4.5  | 1263.9 | 8500.0 |

### 200 images

| profile | phase   | p50 ms ± CI |    p95 |     max |
| :------ | :------ | :---------- | -----: | ------: |
| upload  | submit  | 9.0 ±2.0    |   20.5 |    29.0 |
| upload  | visible | 870.5 ±2.5  | 1726.4 | 17522.0 |

## Memory and storage footprint

Peak process memory and store sizes at the end of each cell, one row per configuration, sorted by peak memory. Use it to spot allocations that scale with image count or scenario rather than with the measured work; a row far above its neighbours is worth a second look.

| images | profile             | peak MiB | temp_db MB | index DB MB | cache DB MB | compressed MB |
| -----: | :------------------ | -------: | ---------: | ----------: | ----------: | ------------: |
|    200 | storm-read-16n      |    534.2 |      165.7 |         1.2 |         1.1 |           7.8 |
|    200 | storm-edit-16n      |    409.3 |        1.1 |         0.9 |         0.6 |           7.8 |
|    200 | storm-read-8        |    409.1 |       41.4 |         1.0 |         1.1 |           7.7 |
|    100 | storm-read-16n      |    408.1 |       20.7 |         0.5 |         0.6 |           3.7 |
|    100 | storm-edit-16n      |    400.9 |        0.6 |         0.6 |         0.6 |           4.0 |
|     50 | storm-edit-16n      |    394.8 |        0.6 |         0.4 |         0.6 |           2.2 |
|    200 | upload              |    392.3 |       10.4 |         0.7 |         1.1 |           7.8 |
|    200 | control             |    381.9 |        4.2 |         1.2 |         0.6 |           7.8 |
|    200 | storm-thumb-16n     |    381.8 |        1.1 |         1.2 |         0.6 |           7.8 |
|    100 | upload              |    380.6 |        2.3 |         0.7 |         0.6 |           4.8 |
|    100 | storm-read-8        |    380.5 |       10.4 |         0.7 |         0.6 |           3.8 |
|    100 | storm-thumb-16n     |    380.5 |        1.1 |         0.5 |         0.6 |           3.8 |
|     50 | upload              |    376.5 |        1.3 |         0.5 |         0.6 |           2.2 |
|     50 | storm-read-16n      |    374.8 |        5.2 |         0.4 |         0.6 |           2.2 |
|    200 | storm-edit-8        |    374.6 |        1.1 |         0.9 |         0.6 |           7.7 |
|    100 | control             |    374.5 |        1.1 |         0.5 |         0.6 |           4.0 |
|     50 | storm-thumb-16n     |    373.5 |        1.1 |         0.6 |         0.6 |           2.2 |
|     50 | storm-read-8        |    373.1 |        2.6 |         0.4 |         0.6 |           2.1 |
|    100 | storm-edit-8        |    372.7 |        1.1 |         0.4 |         0.6 |           3.8 |
|    200 | storm-thumb-8       |    372.1 |        1.1 |         0.7 |         0.6 |           7.7 |
|     50 | storm-edit-8        |    371.7 |        1.1 |         0.4 |         0.6 |           2.1 |
|    100 | storm-thumb-8       |    371.5 |        1.1 |         0.6 |         0.6 |           3.8 |
|     50 | storm-thumb-8       |    370.7 |        1.1 |         1.1 |         0.6 |           2.2 |
|     50 | control             |    370.2 |        1.1 |         0.4 |         0.6 |           2.2 |
|    200 | storm-idle-read-8   |    367.3 |        1.1 |         0.7 |         1.1 |           7.7 |
|    200 | storm-idle-read-16n |    367.2 |       10.4 |         1.0 |         0.6 |           7.8 |
|    100 | storm-idle-read-16n |    366.6 |        2.4 |         0.7 |         0.6 |           4.0 |
|     50 | storm-idle-read-8   |    366.1 |        1.1 |         0.4 |         1.1 |           2.2 |
|    100 | storm-idle-read-8   |    366.1 |        1.1 |         1.1 |         1.1 |           3.9 |
|     50 | storm-idle-read-16n |    365.4 |        1.2 |         1.1 |         0.2 |           2.2 |

## Failed operations

Operations that did not return 200. A non-zero count makes that cell's latency rows suspect.

### 50 images

| profile             | background | access   | failed |
| :------------------ | :--------- | :------- | -----: |
| storm-edit-16n      | scan       | edit     |      0 |
| storm-edit-8        | scan       | edit     |      0 |
| storm-idle-read-16n | idle       | prefetch |      0 |
| storm-idle-read-8   | idle       | prefetch |      0 |
| storm-read-16n      | scan       | prefetch |      0 |
| storm-read-8        | scan       | prefetch |      0 |
| storm-thumb-16n     | scan       | thumb    |      0 |
| storm-thumb-8       | scan       | thumb    |      0 |

### 100 images

| profile             | background | access   | failed |
| :------------------ | :--------- | :------- | -----: |
| storm-edit-16n      | scan       | edit     |      0 |
| storm-edit-8        | scan       | edit     |      0 |
| storm-idle-read-16n | idle       | prefetch |      0 |
| storm-idle-read-8   | idle       | prefetch |      0 |
| storm-read-16n      | scan       | prefetch |      0 |
| storm-read-8        | scan       | prefetch |      0 |
| storm-thumb-16n     | scan       | thumb    |      0 |
| storm-thumb-8       | scan       | thumb    |      0 |

### 200 images

| profile             | background | access   | failed |
| :------------------ | :--------- | :------- | -----: |
| storm-edit-16n      | scan       | edit     |      0 |
| storm-edit-8        | scan       | edit     |      0 |
| storm-idle-read-16n | idle       | prefetch |      0 |
| storm-idle-read-8   | idle       | prefetch |      0 |
| storm-read-16n      | scan       | prefetch |      0 |
| storm-read-8        | scan       | prefetch |      0 |
| storm-thumb-16n     | scan       | thumb    |      0 |
| storm-thumb-8       | scan       | thumb    |      0 |

## Terms

- **p50**: median of the pooled samples for that condition.
- **p95**: 95th percentile of the same pooled samples.
- **max**: largest observed sample.
- **± CI**: half the width of the seeded 95% bootstrap interval, shown next to p50 only; `±n/a` means fewer than two samples.
- **profile**: the cell's scenario, including its client count for storms.
- **coverage**: valid measurement wall as a share of scan wall.
- **peak MiB**: peak resident set; the other footprint columns are apparent store sizes in decimal MB.
