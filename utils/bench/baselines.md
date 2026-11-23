# Benchmark baselines

## Run environment

Host and build of the recorded runs, printed once from the earliest run after checking every run agrees on host, CPU, memory and git identity; clock frequency and load are not compared. `16n` client counts are resolved from the recorded CPU count.

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 2400/2400 MHz, governor powersave, turbo disabled
- **memory**: 63 GiB
- **load average (1/5/15 min)**: 17.57/17.95/15.39 at start, 14.90/16.67/16.71 at end
- **git**: perf/new-baseline2 @ 8cd567b6a4b2
- **captured**: 2026-11-22T18:58:32+00:00 to 2026-11-22T19:10:14+00:00

## Terms

- **p50**: median of the pooled samples for that condition.
- **p95**: 95th percentile of the same pooled samples.
- **max**: largest observed sample.
- **± CI**: half the width of the seeded 95% bootstrap interval, shown next to p50 only; `±n/a` means fewer than two samples.
- **background**: `idle` (no scan, server settled) or `scan` (an album scan is running).
- **foreground**: the access being timed (read, edit, thumbnail, scroll, upload, storm).
- **profile**: the execution cell that produces the foreground samples; storm profiles also name the kind and client count.
- **peak VMM**: process memory high-water mark, shown in MB like the store sizes.

## Measurement validity

One row per load profile with the at-rest and scanning measurements side by side. `accesses` is the number of timed foreground operations pooled for that background — every probe request, every storm op, or every uploaded file — and is the sample count the percentiles rest on. `invalid` counts measurement phases that their guard rejected — a probe round, warmup or storm run whose server state at start or end was not the one the cell claims (the scan finished early, or was still running in an idle cell), or whose status counters could not be read — and those phases are not pooled; the recorded reasons are the cell's `phase_invalid_reasons` field in `baselines.tsv`. `failed` counts operations that did not return 200, and a non-zero `invalid` or `failed` makes the cell's numbers suspect.

### 50 images

| profile         | idle accesses | idle invalid | idle failed | scan accesses | scan invalid | scan failed |
| :-------------- | ------------: | -----------: | ----------: | ------------: | -----------: | ----------: |
| probe           |            24 |          n/a |         n/a |           458 |            0 |         n/a |
| storm-read-8    |            80 |            0 |           0 |         12365 |            0 |           0 |
| storm-read-16n  |          1280 |            0 |           0 |         15622 |            0 |           0 |
| storm-thumb-8   |            80 |            0 |           0 |          7160 |            0 |           0 |
| storm-thumb-16n |          1280 |            0 |           0 |          4770 |            0 |           0 |
| storm-edit-8    |            80 |            0 |           0 |          4319 |            0 |           0 |
| storm-edit-16n  |          1280 |            0 |           0 |          2936 |            0 |           0 |
| upload-loop     |            20 |            0 |         n/a |            20 |            0 |         n/a |

### 100 images

| profile         | idle accesses | idle invalid | idle failed | scan accesses | scan invalid | scan failed |
| :-------------- | ------------: | -----------: | ----------: | ------------: | -----------: | ----------: |
| probe           |            24 |          n/a |         n/a |           280 |            0 |         n/a |
| storm-read-8    |            80 |            0 |           0 |         21311 |            0 |           0 |
| storm-read-16n  |          1280 |            0 |           0 |         27037 |            0 |           0 |
| storm-thumb-8   |            80 |            0 |           0 |         11013 |            0 |           0 |
| storm-thumb-16n |          1280 |            0 |           0 |         23203 |            0 |           0 |
| storm-edit-8    |            80 |            0 |           0 |          5562 |            0 |           0 |
| storm-edit-16n  |          1280 |            0 |           0 |          9205 |            0 |           0 |
| upload-loop     |            20 |            0 |         n/a |            20 |            0 |         n/a |

### 200 images

| profile         | idle accesses | idle invalid | idle failed | scan accesses | scan invalid | scan failed |
| :-------------- | ------------: | -----------: | ----------: | ------------: | -----------: | ----------: |
| probe           |            24 |          n/a |         n/a |           280 |            0 |         n/a |
| storm-read-8    |            80 |            0 |           0 |         41641 |            0 |           0 |
| storm-read-16n  |          1280 |            0 |           0 |         45517 |            0 |           0 |
| storm-thumb-8   |            80 |            0 |           0 |         21165 |            0 |           0 |
| storm-thumb-16n |          1280 |            0 |           0 |         41497 |            0 |           0 |
| storm-edit-8    |            80 |            0 |           0 |         14751 |            0 |           0 |
| storm-edit-16n  |          1280 |            0 |           0 |         15149 |            0 |           0 |
| upload-loop     |            20 |            0 |         n/a |            20 |            0 |         n/a |

## Foreground latency

One row per load profile: the access, its probe or storm client count, and its upload phase. The at-rest (`idle`) and during-scan (`scan`) measurements sit side by side; the difference between them is the indexing-contention cost.

### 50 images

| profile           | idle p50 ms ± CI |    p95 |    max | scan p50 ms ± CI |     p95 |     max |
| :---------------- | :--------------- | -----: | -----: | :--------------- | ------: | ------: |
| probe-read        | 6.2 ±1.8         |    7.1 |    7.2 | 0.9 ±0.4         |     6.6 |    17.1 |
| storm-read-8      | 2.9 ±0.8         |   11.0 |   24.1 | 3.5 ±0.1         |    12.7 |    86.4 |
| storm-read-16n    | 5.2 ±0.3         |   18.2 |   54.5 | 4.3 ±0.1         |    30.3 |   223.5 |
| probe-edit        | 5.4 ±1.2         |    5.7 |    5.7 | 3.8 ±0.3         |     7.3 |     8.7 |
| storm-edit-8      | 14.8 ±3.1        |   32.7 |   49.8 | 13.9 ±0.1        |    30.8 |    68.5 |
| storm-edit-16n    | 319.4 ±4.2       |  645.9 | 1091.4 | 251.8 ±2.5       |   838.0 |  2160.3 |
| probe-thumb       | 6.3 ±2.6         |    6.6 |    6.6 | 2.9 ±0.9         |    28.7 |    51.7 |
| storm-thumb-8     | 6.2 ±1.1         |   27.6 |   39.3 | 8.3 ±0.2         |    27.6 |    82.1 |
| storm-thumb-16n   | 32.5 ±3.2        |  154.5 |  312.0 | 146.8 ±4.8       |   461.8 |   739.7 |
| probe-scroll-bar  | 0.7 ±n/a         |    0.7 |    0.7 | 0.8 ±0.2         |     4.1 |     6.7 |
| probe-scroll-rows | 0.8 ±0.1         |    3.1 |    4.3 | 0.8 ±0.1         |     4.1 |     4.9 |
| probe-scroll-data | 1.5 ±1.9         |    4.6 |    4.6 | 1.6 ±0.4         |     6.0 |    10.0 |
| upload-submit     | 13.0 ±5.2        |   24.6 |   36.0 | 4.0 ±3.0         |    15.8 |    50.0 |
| upload-visible    | 2522.5 ±86.2     | 2760.2 | 2992.0 | 18222.5 ±1718.5  | 20959.5 | 20970.0 |

### 100 images

| profile           | idle p50 ms ± CI |    p95 |    max | scan p50 ms ± CI |     p95 |     max |
| :---------------- | :--------------- | -----: | -----: | :--------------- | ------: | ------: |
| probe-read        | 12.2 ±9.8        |   28.5 |   30.3 | 0.9 ±0.8         |    13.1 |    20.1 |
| storm-read-8      | 4.0 ±1.0         |   16.2 |   21.4 | 3.6 ±0.0         |    13.7 |    79.7 |
| storm-read-16n    | 7.7 ±0.4         |   32.7 |  211.1 | 4.9 ±0.1         |    36.8 |   795.2 |
| probe-edit        | 4.5 ±3.8         |   10.1 |   10.8 | 3.1 ±0.6         |     7.8 |    13.0 |
| storm-edit-8      | 14.5 ±1.1        |   27.0 |   38.6 | 21.4 ±0.3        |    47.9 |   233.1 |
| storm-edit-16n    | 258.6 ±1.1       |  519.8 | 1032.1 | 239.5 ±1.2       |   506.0 |  1140.6 |
| probe-thumb       | 2.8 ±1.2         |    3.6 |    3.7 | 4.6 ±2.5         |    12.7 |    12.8 |
| storm-thumb-8     | 5.5 ±1.1         |   15.9 |   24.2 | 9.2 ±0.2         |    40.6 |   114.6 |
| storm-thumb-16n   | 11.5 ±0.6        |   52.5 |  103.3 | 17.5 ±0.4        |   154.3 |   450.8 |
| probe-scroll-bar  | 0.8 ±n/a         |    0.8 |    0.8 | 0.8 ±0.1         |     1.0 |     1.7 |
| probe-scroll-rows | 0.8 ±1.0         |    3.5 |    3.8 | 0.8 ±0.2         |     3.7 |     7.2 |
| probe-scroll-data | 6.0 ±2.8         |    8.3 |    8.4 | 1.4 ±0.8         |     7.3 |    11.1 |
| upload-submit     | 6.5 ±4.5         |   29.1 |   68.0 | 4.5 ±2.0         |    23.2 |    46.0 |
| upload-visible    | 2362.5 ±105.8    | 2586.9 | 2623.0 | 22098.5 ±1407.8  | 23966.0 | 23966.0 |

### 200 images

| profile           | idle p50 ms ± CI |    p95 |    max | scan p50 ms ± CI |     p95 |     max |
| :---------------- | :--------------- | -----: | -----: | :--------------- | ------: | ------: |
| probe-read        | 10.9 ±8.8        |   26.7 |   28.4 | 0.9 ±0.3         |     6.3 |    21.4 |
| storm-read-8      | 3.3 ±1.0         |   50.7 |   88.4 | 3.7 ±0.0         |    15.5 |   184.1 |
| storm-read-16n    | 7.3 ±0.5         |  217.8 |  316.9 | 5.7 ±0.1         |   145.7 |  2368.0 |
| probe-edit        | 5.7 ±2.4         |    9.2 |    9.5 | 3.4 ±0.9         |    17.2 |    71.1 |
| storm-edit-8      | 14.7 ±1.9        |   29.8 |   38.7 | 16.6 ±0.1        |    37.1 |   327.3 |
| storm-edit-16n    | 250.8 ±1.0       |  514.7 | 1024.6 | 267.6 ±1.0       |   567.4 |  1439.9 |
| probe-thumb       | 2.1 ±10.3        |   20.1 |   22.1 | 1.6 ±1.2         |     6.1 |    15.8 |
| storm-thumb-8     | 4.7 ±1.0         |   14.6 |   16.5 | 9.9 ±0.1         |    36.9 |    99.7 |
| storm-thumb-16n   | 14.4 ±1.0        |   75.4 |  162.5 | 56.2 ±1.0        |   221.5 |   583.1 |
| probe-scroll-bar  | 1.2 ±n/a         |    1.2 |    1.2 | 0.7 ±0.1         |     1.6 |     1.8 |
| probe-scroll-rows | 0.8 ±0.1         |    0.8 |    0.9 | 0.7 ±0.0         |     1.4 |     4.4 |
| probe-scroll-data | 2.7 ±1.4         |    3.5 |    3.6 | 0.8 ±0.5         |     6.2 |     8.7 |
| upload-submit     | 12.0 ±7.8        |   50.4 |   57.0 | 4.5 ±3.5         |    65.0 |   578.0 |
| upload-visible    | 2644.0 ±91.1     | 2701.0 | 2891.0 | 49775.0 ±585.8   | 52065.1 | 52066.0 |

## Scan time

Wall clock of the populate scan and the resulting ingest rate for a few load profiles, one column per image count. The no-load row is the median populate scan of the idle-storm cells, which scan with no foreground traffic; the storm rows add the named client load; the upload row adds a serial upload loop (submit, then poll visibility). Each cell contributes one scan wall, so treat small differences between rows as noise.

| load profile    |          50 images |         100 images |         200 images |
| :-------------- | -----------------: | -----------------: | -----------------: |
| no-load         | 12.4 s (4.0 img/s) | 18.5 s (5.4 img/s) | 42.3 s (4.7 img/s) |
| storm-read-8    | 11.7 s (4.3 img/s) | 20.1 s (5.0 img/s) | 41.0 s (4.9 img/s) |
| storm-read-16n  | 12.8 s (3.9 img/s) | 20.7 s (4.8 img/s) | 40.4 s (5.0 img/s) |
| storm-thumb-8   | 11.2 s (4.5 img/s) | 22.4 s (4.5 img/s) | 38.5 s (5.2 img/s) |
| storm-thumb-16n | 10.8 s (4.6 img/s) | 20.3 s (4.9 img/s) | 40.0 s (5.0 img/s) |
| storm-edit-8    |  8.9 s (5.6 img/s) | 17.5 s (5.7 img/s) | 36.2 s (5.5 img/s) |
| storm-edit-16n  |  9.6 s (5.2 img/s) | 19.4 s (5.2 img/s) | 34.7 s (5.8 img/s) |
| upload-loop     | 15.0 s (3.3 img/s) | 22.7 s (4.4 img/s) | 50.8 s (3.9 img/s) |

## Memory and storage footprint

Peak process memory and store sizes at the end of each cell, one table per metric with profile rows and image-count columns, sorted by peak VMM. All values are in MB. Footprint is expected to be broadly consistent across access types at the same image count; a large outlier marks a possible bottleneck or an optimization target, and growth with image count that outpaces the library size is likewise worth investigating.

| profile              | peak VMM (50) | peak VMM (100) | peak VMM (200) |
| :------------------- | ------------: | -------------: | -------------: |
| scan-upload          |         703.1 |          706.2 |          716.6 |
| scan-storm-read-16n  |         394.9 |          422.3 |          549.7 |
| scan-storm-read-8    |         391.7 |          402.1 |          432.5 |
| scan-storm-edit-16n  |         413.7 |          417.9 |          425.8 |
| scan-storm-thumb-16n |         402.3 |          402.0 |          407.1 |
| idle-upload          |         386.0 |          388.7 |          397.8 |
| scan-storm-edit-8    |         389.9 |          390.3 |          392.6 |
| scan-storm-thumb-8   |         388.6 |          389.0 |          390.4 |
| probe                |         387.9 |          388.1 |          388.0 |
| idle-storm-thumb-8   |         383.4 |          384.1 |          385.4 |
| idle-storm-read-8    |         383.3 |          384.4 |          385.2 |
| idle-storm-edit-8    |         383.4 |          384.1 |          385.1 |
| idle-storm-thumb-16n |         383.4 |          384.3 |          385.0 |
| idle-storm-read-16n  |         383.7 |          384.3 |          384.7 |
| idle-storm-edit-16n  |         383.4 |          384.8 |          384.5 |

| profile              | compressed (50) | compressed (100) | compressed (200) |
| :------------------- | --------------: | ---------------: | ---------------: |
| scan-upload          |             3.0 |              4.7 |              8.6 |
| scan-storm-read-16n  |             2.1 |              3.9 |              7.8 |
| scan-storm-read-8    |             2.0 |              3.8 |              7.5 |
| scan-storm-edit-16n  |             2.2 |              4.0 |              7.7 |
| scan-storm-thumb-16n |             2.2 |              3.8 |              7.7 |
| idle-upload          |             3.0 |              4.7 |              8.6 |
| scan-storm-edit-8    |             2.0 |              3.8 |              7.8 |
| scan-storm-thumb-8   |             2.1 |              3.8 |              7.6 |
| probe                |             2.2 |              3.8 |              7.7 |
| idle-storm-thumb-8   |             2.0 |              3.8 |              7.8 |
| idle-storm-read-8    |             2.0 |              3.9 |              7.8 |
| idle-storm-edit-8    |             2.1 |              3.8 |              7.7 |
| idle-storm-thumb-16n |             2.2 |              4.0 |              7.8 |
| idle-storm-read-16n  |             2.2 |              4.0 |              7.8 |
| idle-storm-edit-16n  |             2.2 |              4.0 |              7.8 |

| profile              | index DB (50) | index DB (100) | index DB (200) |
| :------------------- | ------------: | -------------: | -------------: |
| scan-upload          |           1.1 |            1.1 |            0.7 |
| scan-storm-read-16n  |           1.1 |            0.5 |            1.0 |
| scan-storm-read-8    |           0.4 |            1.1 |            1.1 |
| scan-storm-edit-16n  |           0.3 |            0.5 |            1.3 |
| scan-storm-thumb-16n |           1.1 |            1.1 |            0.7 |
| idle-upload          |           0.7 |            0.5 |            1.2 |
| scan-storm-edit-8    |           0.5 |            0.5 |            0.7 |
| scan-storm-thumb-8   |           0.6 |            0.7 |            1.3 |
| probe                |           0.4 |            0.4 |            1.1 |
| idle-storm-thumb-8   |           0.4 |            0.7 |            0.9 |
| idle-storm-read-8    |           0.4 |            0.7 |            1.1 |
| idle-storm-edit-8    |           0.3 |            0.5 |            0.7 |
| idle-storm-thumb-16n |           1.1 |            1.1 |            0.7 |
| idle-storm-read-16n  |           1.1 |            1.1 |            0.7 |
| idle-storm-edit-16n  |           0.3 |            0.6 |            0.7 |

| profile              | temp (50) | temp (100) | temp (200) |
| :------------------- | --------: | ---------: | ---------: |
| scan-upload          |       1.1 |        2.2 |        4.5 |
| scan-storm-read-16n  |       4.5 |       35.7 |      165.7 |
| scan-storm-read-8    |       2.2 |       17.8 |       41.4 |
| scan-storm-edit-16n  |       1.1 |        1.1 |        1.1 |
| scan-storm-thumb-16n |       1.1 |        1.1 |        1.1 |
| idle-upload          |       0.7 |        1.1 |        4.5 |
| scan-storm-edit-8    |       1.1 |        1.1 |        1.1 |
| scan-storm-thumb-8   |       1.1 |        1.1 |        1.1 |
| probe                |       0.6 |        0.6 |        0.6 |
| idle-storm-thumb-8   |       1.1 |        1.1 |        1.1 |
| idle-storm-read-8    |       0.6 |        0.6 |        0.7 |
| idle-storm-edit-8    |       1.1 |        1.1 |        1.1 |
| idle-storm-thumb-16n |       1.1 |        1.1 |        1.1 |
| idle-storm-read-16n  |       1.1 |        2.4 |       10.4 |
| idle-storm-edit-16n  |       1.1 |        1.1 |        1.1 |

| profile              | cache (50) | cache (100) | cache (200) |
| :------------------- | ---------: | ----------: | ----------: |
| scan-upload          |        0.6 |         0.6 |         1.1 |
| scan-storm-read-16n  |        0.6 |         0.8 |         1.7 |
| scan-storm-read-8    |        0.6 |         0.6 |         1.5 |
| scan-storm-edit-16n  |        0.6 |         0.6 |         1.1 |
| scan-storm-thumb-16n |        1.1 |         1.1 |         1.1 |
| idle-upload          |        0.6 |         0.6 |         0.6 |
| scan-storm-edit-8    |        1.1 |         1.1 |         1.1 |
| scan-storm-thumb-8   |        1.1 |         1.1 |         1.1 |
| probe                |        0.6 |         0.6 |         0.6 |
| idle-storm-thumb-8   |        1.1 |         1.1 |         1.1 |
| idle-storm-read-8    |        1.1 |         1.1 |         0.3 |
| idle-storm-edit-8    |        1.1 |         1.1 |         1.1 |
| idle-storm-thumb-16n |        1.1 |         1.1 |         1.1 |
| idle-storm-read-16n  |        0.2 |         0.6 |         0.6 |
| idle-storm-edit-16n  |        1.1 |         1.1 |         1.1 |
