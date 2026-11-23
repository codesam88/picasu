# Benchmark baselines

## Run environment

Host and build of the recorded runs, printed once from the earliest run after checking every run agrees on host, CPU, memory and git identity; clock frequency and load are not compared. `16n` client counts are resolved from the recorded CPU count.

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 2200/2400 MHz, governor powersave, turbo disabled
- **memory**: 63 GiB
- **load average (1/5/15 min)**: 0.69/1.09/1.39 at start, 6.95/6.40/3.71 at end
- **git**: perf/new-baseline2 @ f561b33d9289
- **captured**: 2026-11-23T00:09:22+00:00 to 2026-11-23T00:13:37+00:00

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

One row per load profile with the at-rest and scanning measurements side by side. `accesses` is the number of timed foreground operations pooled for that background — every probe request, every storm op, or every uploaded file — and is the sample count the percentiles rest on. `invalid` counts measurement phases that their guard rejected — a probe round, warmup or storm run whose server state at start or end was not the one the cell claims (the scan finished early, or was still running in an idle cell), or whose status counters could not be read — and those phases are not pooled; the recorded reasons are the cell's `phase_invalid_reasons` field in `baselines.tsv`. `failed` counts operations that did not return 200. A cell whose rejections exceed 10% of its measured probe rounds — the only phase a guard can reject without failing the cell at runtime — or that recorded any failed operation is suspect: the renderer prints a WARNING to stderr, and the validity table reports the cell either way.

### 100 images

| profile         | idle accesses | idle invalid | idle failed | scan accesses | scan invalid | scan failed |
| :-------------- | ------------: | -----------: | ----------: | ------------: | -----------: | ----------: |
| probe           |            24 |          n/a |         n/a |           572 |            0 |         n/a |
| storm-read-8    |            80 |            0 |           0 |         11350 |            0 |           0 |
| storm-read-16n  |          1280 |            0 |           0 |         14566 |            0 |           0 |
| storm-thumb-8   |            80 |            0 |           0 |          8015 |            0 |           0 |
| storm-thumb-16n |          1280 |            0 |           0 |         13201 |            0 |           0 |
| storm-edit-8    |            80 |            0 |           0 |         12306 |            0 |           0 |
| storm-edit-16n  |          1280 |            0 |           0 |         17810 |            0 |           0 |
| upload-loop     |            20 |            0 |         n/a |            20 |            0 |         n/a |

### 200 images

| profile         | idle accesses | idle invalid | idle failed | scan accesses | scan invalid | scan failed |
| :-------------- | ------------: | -----------: | ----------: | ------------: | -----------: | ----------: |
| probe           |            24 |          n/a |         n/a |           560 |            1 |         n/a |
| storm-read-8    |            80 |            0 |           0 |         22939 |            0 |           0 |
| storm-read-16n  |          1280 |            0 |           0 |         29215 |            0 |           0 |
| storm-thumb-8   |            80 |            0 |           0 |         10770 |            0 |           0 |
| storm-thumb-16n |          1280 |            0 |           0 |         28856 |            0 |           0 |
| storm-edit-8    |            80 |            0 |           0 |         25111 |            0 |           0 |
| storm-edit-16n  |          1280 |            0 |           0 |         34244 |            0 |           0 |
| upload-loop     |            20 |            0 |         n/a |            20 |            0 |         n/a |

## Foreground latency

One row per load profile: the access, its probe or storm client count, and its upload phase. The at-rest (`idle`) and during-scan (`scan`) measurements sit side by side; the difference between them is the indexing-contention cost.

### 100 images

| profile           | idle p50 ms ± CI |    p95 |    max | scan p50 ms ± CI |     p95 |     max |
| :---------------- | :--------------- | -----: | -----: | :--------------- | ------: | ------: |
| probe-read        | 6.2 ±0.4         |    6.5 |    6.6 | 0.8 ±0.1         |     6.0 |     8.2 |
| storm-read-8      | 2.0 ±0.3         |    5.2 |   14.9 | 3.3 ±0.0         |    14.9 |    48.4 |
| storm-read-16n    | 2.2 ±0.1         |  139.0 |  186.8 | 3.8 ±0.1         |    98.7 |   314.4 |
| probe-edit        | 2.3 ±0.1         |    2.3 |    2.3 | 1.4 ±0.0         |     2.2 |     3.4 |
| storm-edit-8      | 3.6 ±0.3         |   10.1 |   20.1 | 4.8 ±0.0         |    13.0 |    29.5 |
| storm-edit-16n    | 2.6 ±0.1         |    6.0 |   10.4 | 5.3 ±0.0         |    19.1 |   224.7 |
| probe-thumb       | 2.2 ±0.2         |    2.5 |    2.5 | 1.5 ±0.2         |    10.6 |    24.1 |
| storm-thumb-8     | 5.6 ±0.9         |   13.9 |   22.5 | 8.0 ±0.1         |    18.5 |    39.2 |
| storm-thumb-16n   | 2.5 ±0.1         |   13.9 |   35.9 | 17.8 ±0.4        |    82.2 |   220.7 |
| probe-scroll-bar  | 1.4 ±n/a         |    1.4 |    1.4 | 0.7 ±0.0         |     1.1 |     1.6 |
| probe-scroll-rows | 1.5 ±0.1         |    1.6 |    1.6 | 0.7 ±0.0         |     1.6 |     5.0 |
| probe-scroll-data | 2.4 ±0.8         |    3.1 |    3.1 | 0.9 ±0.0         |     3.4 |     8.6 |
| upload-submit     | 4.0 ±0.5         |   10.4 |   17.0 | 14.5 ±8.2        |   112.3 |   118.0 |
| upload-visible    | 2266.0 ±1.0      | 2277.5 | 2287.0 | 12954.0 ±688.3   | 14181.1 | 14184.0 |

### 200 images

| profile           | idle p50 ms ± CI |    p95 |    max | scan p50 ms ± CI |     p95 |     max |
| :---------------- | :--------------- | -----: | -----: | :--------------- | ------: | ------: |
| probe-read        | 7.9 ±0.7         |    8.6 |    8.7 | 1.9 ±0.5         |    12.2 |    16.6 |
| storm-read-8      | 3.2 ±0.5         |   37.4 |   43.1 | 3.6 ±0.1         |    16.2 |   136.1 |
| storm-read-16n    | 2.0 ±0.1         |  230.8 |  280.3 | 4.5 ±0.0         |   264.2 |  1963.2 |
| probe-edit        | 1.4 ±0.0         |    1.4 |    1.4 | 1.5 ±0.2         |     4.0 |     5.0 |
| storm-edit-8      | 2.9 ±0.2         |    5.2 |    6.3 | 4.5 ±0.0         |    12.3 |    49.7 |
| storm-edit-16n    | 2.7 ±0.1         |    9.7 |   20.0 | 4.8 ±0.0         |    15.9 |    64.0 |
| probe-thumb       | 1.1 ±0.0         |    1.2 |    1.2 | 10.3 ±1.5        |    24.0 |    28.7 |
| storm-thumb-8     | 3.4 ±0.2         |    7.5 |   12.1 | 11.5 ±0.2        |    40.7 |    82.9 |
| storm-thumb-16n   | 2.1 ±0.1         |    7.0 |   25.8 | 18.5 ±0.3        |   137.2 |   333.9 |
| probe-scroll-bar  | 0.7 ±n/a         |    0.7 |    0.7 | 0.8 ±0.2         |     1.9 |     1.9 |
| probe-scroll-rows | 0.8 ±0.1         |    1.3 |    1.4 | 1.1 ±0.5         |     4.0 |     6.4 |
| probe-scroll-data | 1.9 ±0.8         |    2.6 |    2.7 | 1.9 ±0.4         |     5.1 |     8.4 |
| upload-submit     | 11.0 ±4.2        |   26.3 |   32.0 | 4.0 ±0.5         |    20.1 |    22.0 |
| upload-visible    | 2322.5 ±11.5     | 2338.3 | 2345.0 | 21629.5 ±708.8   | 22836.0 | 22836.0 |

## Scan time

Wall clock of the populate scan and the resulting ingest rate for a few load profiles, one column per image count. The no-load row is the median populate scan of the idle-storm cells, which scan with no foreground traffic; the storm rows add the named client load; the upload row adds a serial upload loop (submit, then poll visibility). Each cell contributes one scan wall, so treat small differences between rows as noise.

| load profile    |         100 images |          200 images |
| :-------------- | -----------------: | ------------------: |
| no-load         | 10.4 s (9.6 img/s) | 18.9 s (10.6 img/s) |
| storm-read-8    | 11.2 s (8.9 img/s) |  24.0 s (8.3 img/s) |
| storm-read-16n  | 11.2 s (8.9 img/s) |  25.0 s (8.0 img/s) |
| storm-thumb-8   | 10.3 s (9.7 img/s) |  22.2 s (9.0 img/s) |
| storm-thumb-16n | 11.3 s (8.8 img/s) |  23.8 s (8.4 img/s) |
| storm-edit-8    | 11.8 s (8.5 img/s) |  24.3 s (8.2 img/s) |
| storm-edit-16n  | 14.2 s (7.0 img/s) |  25.8 s (7.8 img/s) |
| upload-loop     | 12.1 s (8.3 img/s) |  21.8 s (9.2 img/s) |

## Memory and storage footprint

Peak process memory and store sizes at the end of each cell, one table per metric with profile rows and image-count columns, rows in a fixed profile order. All values are in MB. Footprint is expected to be broadly consistent across access types at the same image count; a large outlier marks a possible bottleneck or an optimization target, and growth with image count that outpaces the library size is likewise worth investigating.

| profile              | peak VMM (100) | peak VMM (200) |
| :------------------- | -------------: | -------------: |
| idle-storm-edit-16n  |          384.2 |          384.6 |
| idle-storm-edit-8    |          383.8 |          384.9 |
| idle-storm-read-16n  |          383.7 |          385.1 |
| idle-storm-read-8    |          383.8 |          385.0 |
| idle-storm-thumb-16n |          384.1 |          384.9 |
| idle-storm-thumb-8   |          384.5 |          385.1 |
| idle-upload          |          387.3 |          393.1 |
| probe                |          390.2 |          392.7 |
| scan-storm-edit-16n  |          405.6 |          403.0 |
| scan-storm-edit-8    |          391.4 |          392.1 |
| scan-storm-read-16n  |          442.9 |          553.0 |
| scan-storm-read-8    |          402.5 |          432.4 |
| scan-storm-thumb-16n |          398.6 |          404.3 |
| scan-storm-thumb-8   |          389.1 |          389.9 |
| scan-upload          |          714.8 |          713.3 |

| profile              | compressed (100) | compressed (200) |
| :------------------- | ---------------: | ---------------: |
| idle-storm-edit-16n  |              4.0 |              7.8 |
| idle-storm-edit-8    |              4.0 |              7.8 |
| idle-storm-read-16n  |              4.0 |              7.8 |
| idle-storm-read-8    |              4.0 |              7.8 |
| idle-storm-thumb-16n |              4.0 |              7.8 |
| idle-storm-thumb-8   |              4.0 |              7.8 |
| idle-upload          |              4.7 |              8.6 |
| probe                |              4.0 |              7.8 |
| scan-storm-edit-16n  |              4.0 |              7.8 |
| scan-storm-edit-8    |              4.0 |              7.8 |
| scan-storm-read-16n  |              4.0 |              7.8 |
| scan-storm-read-8    |              4.0 |              7.8 |
| scan-storm-thumb-16n |              4.0 |              7.8 |
| scan-storm-thumb-8   |              4.0 |              7.8 |
| scan-upload          |              4.7 |              8.6 |

| profile              | index DB (100) | index DB (200) |
| :------------------- | -------------: | -------------: |
| idle-storm-edit-16n  |            0.7 |            0.7 |
| idle-storm-edit-8    |            0.6 |            0.7 |
| idle-storm-read-16n  |            0.6 |            0.7 |
| idle-storm-read-8    |            0.5 |            1.1 |
| idle-storm-thumb-16n |            0.6 |            1.0 |
| idle-storm-thumb-8   |            1.1 |            0.7 |
| idle-upload          |            0.7 |            0.7 |
| probe                |            0.5 |            0.9 |
| scan-storm-edit-16n  |            0.8 |            0.8 |
| scan-storm-edit-8    |            0.5 |            0.7 |
| scan-storm-read-16n  |            0.5 |            1.2 |
| scan-storm-read-8    |            0.7 |            0.8 |
| scan-storm-thumb-16n |            0.7 |            0.9 |
| scan-storm-thumb-8   |            0.6 |            0.7 |
| scan-upload          |            0.5 |            1.3 |

| profile              | temp (100) | temp (200) |
| :------------------- | ---------: | ---------: |
| idle-storm-edit-16n  |        1.1 |        1.1 |
| idle-storm-edit-8    |        1.1 |        1.1 |
| idle-storm-read-16n  |        9.8 |       10.4 |
| idle-storm-read-8    |        0.6 |        0.7 |
| idle-storm-thumb-16n |        1.1 |        1.1 |
| idle-storm-thumb-8   |        1.1 |        1.1 |
| idle-upload          |        1.1 |        2.1 |
| probe                |        0.6 |        1.1 |
| scan-storm-edit-16n  |        1.1 |        1.1 |
| scan-storm-edit-8    |        1.1 |        1.1 |
| scan-storm-read-16n  |       35.7 |      142.6 |
| scan-storm-read-8    |       17.8 |       35.7 |
| scan-storm-thumb-16n |        1.1 |        1.1 |
| scan-storm-thumb-8   |        1.1 |        1.1 |
| scan-upload          |        1.1 |        2.2 |

| profile              | cache (100) | cache (200) |
| :------------------- | ----------: | ----------: |
| idle-storm-edit-16n  |         1.1 |         1.1 |
| idle-storm-edit-8    |         1.1 |         1.1 |
| idle-storm-read-16n  |         0.2 |         0.1 |
| idle-storm-read-8    |         1.1 |         1.1 |
| idle-storm-thumb-16n |         1.1 |         1.1 |
| idle-storm-thumb-8   |         1.1 |         1.1 |
| idle-upload          |         0.6 |         0.6 |
| probe                |         0.6 |         0.6 |
| scan-storm-edit-16n  |         1.1 |         1.1 |
| scan-storm-edit-8    |         1.1 |         1.1 |
| scan-storm-read-16n  |         0.6 |         1.1 |
| scan-storm-read-8    |         0.6 |         1.1 |
| scan-storm-thumb-16n |         1.1 |         1.1 |
| scan-storm-thumb-8   |         1.1 |         1.1 |
| scan-upload          |         0.6 |         0.6 |
