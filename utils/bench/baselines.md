# Benchmark baselines

## Run environment

Host and build of the recorded runs, printed once from the earliest run after
checking every run agrees on host, CPU, memory and git identity; clock
frequency and load are not compared. `16n` client counts are resolved from the
recorded CPU count.

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 1800/2400 MHz,
  governor powersave, turbo disabled
- **memory**: 63 GiB
- **load average (1/5/15 min)**: 0.61/0.81/1.61 at start, 4.98/4.29/2.99 at
  end
- **git**: fix/album-concurrency-baseline2 @ b30fcaf0f7e6
- **captured**: 2026-11-23T10:05:01+00:00 to 2026-11-23T10:09:07+00:00

## Terms

- **p50**: median of the pooled samples for that condition.
- **p95**: 95th percentile of the same pooled samples.
- **max**: largest observed sample.
- **± CI**: half the width of the seeded 95% bootstrap interval, shown next to
  p50 only; `±n/a` means fewer than two samples.
- **background**: `idle` (no scan, server settled) or `scan` (an album scan is
  running).
- **foreground**: the access being timed (read, edit, thumbnail, scroll,
  upload, storm).
- **profile**: the execution cell that produces the foreground samples; storm
  profiles also name the kind and client count.
- **peak VMM**: process memory high-water mark, shown in MB like the store
  sizes.

## Measurement validity

One row per load profile with the at-rest and scanning measurements side by
side. `accesses` is the number of timed foreground operations pooled for that
background — every probe request, every storm op, or every uploaded file — and
is the sample count the percentiles rest on. `invalid` counts measurement
phases that their guard rejected — a probe round, warmup or storm run whose
server state at start or end was not the one the cell claims (the scan
finished early, or was still running in an idle cell), or whose status
counters could not be read — and those phases are not pooled; the recorded
reasons are the cell's `phase_invalid_reasons` field in `baselines.tsv`.
`failed` counts operations that did not return 200. A cell whose rejections
exceed 10% of its measured probe rounds — the only phase a guard can reject
without failing the cell at runtime — or that recorded any failed operation is
suspect: the renderer prints a WARNING to stderr, and the validity table
reports the cell either way.

### 100 images

| profile         | idle accesses | idle invalid | idle failed | scan accesses | scan invalid | scan failed |
| :-------------- | ------------: | -----------: | ----------: | ------------: | -----------: | ----------: |
| probe           |            24 |          n/a |         n/a |           575 |            0 |         n/a |
| storm-read-8    |            80 |            0 |           0 |         11208 |            0 |           0 |
| storm-read-16n  |          1280 |            0 |           0 |         13901 |            0 |           0 |
| storm-thumb-8   |            80 |            0 |           0 |          6163 |            0 |           0 |
| storm-thumb-16n |          1280 |            0 |           0 |         13672 |            0 |           0 |
| storm-edit-8    |            80 |            0 |           0 |         11385 |            0 |           0 |
| storm-edit-16n  |          1280 |            0 |           0 |         14064 |            0 |           0 |
| upload-loop     |            20 |            0 |         n/a |            20 |            0 |         n/a |

### 200 images

| profile         | idle accesses | idle invalid | idle failed | scan accesses | scan invalid | scan failed |
| :-------------- | ------------: | -----------: | ----------: | ------------: | -----------: | ----------: |
| probe           |            24 |          n/a |         n/a |           560 |            0 |         n/a |
| storm-read-8    |            80 |            0 |           0 |         20174 |            0 |           0 |
| storm-read-16n  |          1280 |            0 |           0 |         25728 |            0 |           0 |
| storm-thumb-8   |            80 |            0 |           0 |         14428 |            0 |           0 |
| storm-thumb-16n |          1280 |            0 |           0 |         26061 |            0 |           0 |
| storm-edit-8    |            80 |            0 |           0 |         22554 |            0 |           0 |
| storm-edit-16n  |          1280 |            0 |           0 |         29236 |            0 |           0 |
| upload-loop     |            20 |            0 |         n/a |            20 |            0 |         n/a |

## Foreground latency

One row per load profile: the access, its probe or storm client count, and its
upload phase. The at-rest (`idle`) and during-scan (`scan`) measurements sit
side by side; the difference between them is the indexing-contention cost.

### 100 images

| profile           | idle p50 ms ± CI |    p95 |    max | scan p50 ms ± CI |     p95 |     max |
| :---------------- | :--------------- | -----: | -----: | :--------------- | ------: | ------: |
| probe-read        | 3.5 ±0.4         |    4.1 |    4.1 | 1.4 ±0.7         |     7.7 |     8.9 |
| storm-read-8      | 2.1 ±0.2         |    3.8 |    4.7 | 3.1 ±0.1         |    14.8 |    40.2 |
| storm-read-16n    | 3.7 ±0.1         |   37.0 |  112.7 | 3.3 ±0.1         |    61.7 |   254.4 |
| probe-edit        | 1.2 ±0.2         |    1.4 |    1.5 | 1.4 ±0.1         |     3.0 |     4.0 |
| storm-edit-8      | 2.3 ±0.1         |    3.7 |    5.3 | 4.8 ±0.0         |    13.1 |    38.3 |
| storm-edit-16n    | 3.7 ±0.1         |    8.2 |   17.3 | 4.3 ±0.1         |    15.8 |    44.5 |
| probe-thumb       | 1.2 ±0.2         |    1.4 |    1.5 | 3.0 ±0.7         |    20.6 |    31.9 |
| storm-thumb-8     | 2.5 ±0.3         |    4.2 |    5.2 | 9.4 ±0.2         |    33.8 |    98.6 |
| storm-thumb-16n   | 3.0 ±0.1         |    6.4 |   11.1 | 18.8 ±0.4        |   137.1 |   387.1 |
| probe-scroll-bar  | 0.7 ±n/a         |    0.7 |    0.7 | 0.7 ±0.1         |     1.7 |     1.8 |
| probe-scroll-rows | 0.8 ±0.1         |    0.8 |    0.8 | 0.7 ±0.1         |     4.0 |     4.5 |
| probe-scroll-data | 1.3 ±0.4         |    1.5 |    1.5 | 1.4 ±0.3         |     5.0 |     7.8 |
| upload-submit     | 10.0 ±1.8        |   31.3 |   56.0 | 4.5 ±0.8         |    69.8 |    84.0 |
| upload-visible    | 2283.0 ±3.2      | 2304.5 | 2333.0 | 13056.0 ±579.8   | 14089.0 | 14090.0 |

### 200 images

| profile           | idle p50 ms ± CI |    p95 |    max | scan p50 ms ± CI |     p95 |     max |
| :---------------- | :--------------- | -----: | -----: | :--------------- | ------: | ------: |
| probe-read        | 6.0 ±0.5         |    6.2 |    6.2 | 1.2 ±1.1         |    11.5 |    13.7 |
| storm-read-8      | 2.0 ±0.2         |    3.9 |    4.6 | 3.1 ±0.0         |    17.0 |    86.2 |
| storm-read-16n    | 2.0 ±0.1         |  254.0 |  305.1 | 4.5 ±0.0         |   266.4 |   607.7 |
| probe-edit        | 1.2 ±0.1         |    1.2 |    1.2 | 1.4 ±0.1         |     2.9 |     7.7 |
| storm-edit-8      | 3.6 ±0.6         |    6.5 |    8.0 | 4.7 ±0.1         |    12.9 |    35.4 |
| storm-edit-16n    | 4.7 ±0.2         |   10.7 |   47.0 | 4.6 ±0.0         |    16.3 |    60.8 |
| probe-thumb       | 1.2 ±0.1         |    1.4 |    1.4 | 4.0 ±1.5         |    22.0 |    39.3 |
| storm-thumb-8     | 2.4 ±0.2         |    4.4 |    6.3 | 8.7 ±0.1         |    19.8 |    53.5 |
| storm-thumb-16n   | 3.8 ±0.1         |    7.1 |   20.7 | 17.4 ±0.2        |    69.9 |   157.9 |
| probe-scroll-bar  | 0.6 ±n/a         |    0.6 |    0.6 | 0.7 ±0.1         |     1.8 |     1.9 |
| probe-scroll-rows | 0.7 ±0.1         |    0.8 |    0.8 | 0.7 ±0.0         |     3.0 |     5.0 |
| probe-scroll-data | 1.8 ±0.9         |    2.6 |    2.7 | 1.7 ±0.3         |     6.0 |     8.0 |
| upload-submit     | 10.5 ±1.5        |   16.2 |   38.0 | 3.0 ±0.5         |    14.6 |    26.0 |
| upload-visible    | 2318.5 ±5.2      | 2337.2 | 2359.0 | 21118.0 ±635.0   | 22595.3 | 22601.0 |

## Scan time

Wall clock of the populate scan and the resulting ingest rate for a few load
profiles, one column per image count. The no-load row is the median populate
scan of the idle-storm cells, which scan with no foreground traffic; the storm
rows add the named client load; the upload row adds a serial upload loop
(submit, then poll visibility). Each cell contributes one scan wall, so treat
small differences between rows as noise.

| load profile    |         100 images |          200 images |
| :-------------- | -----------------: | ------------------: |
| no-load         | 9.7 s (10.3 img/s) | 18.2 s (11.0 img/s) |
| storm-read-8    | 10.4 s (9.6 img/s) | 19.4 s (10.3 img/s) |
| storm-read-16n  | 10.8 s (9.3 img/s) |  22.2 s (9.0 img/s) |
| storm-thumb-8   | 10.6 s (9.4 img/s) | 19.3 s (10.4 img/s) |
| storm-thumb-16n | 11.8 s (8.5 img/s) |  20.7 s (9.7 img/s) |
| storm-edit-8    | 10.7 s (9.3 img/s) |  20.9 s (9.6 img/s) |
| storm-edit-16n  | 11.0 s (9.1 img/s) |  21.4 s (9.3 img/s) |
| upload-loop     | 11.5 s (8.7 img/s) |  21.6 s (9.3 img/s) |

## Memory and storage footprint

Peak process memory and store sizes at the end of each cell, one table per
metric with profile rows and image-count columns, rows in a fixed profile
order. All values are in MB. Footprint is expected to be broadly consistent
across access types at the same image count; a large outlier marks a possible
bottleneck or an optimization target, and growth with image count that
outpaces the library size is likewise worth investigating.

| profile              | peak VMM (100) | peak VMM (200) |
| :------------------- | -------------: | -------------: |
| idle-storm-edit-16n  |          384.2 |          385.9 |
| idle-storm-edit-8    |          384.3 |          385.1 |
| idle-storm-read-16n  |          384.4 |          385.1 |
| idle-storm-read-8    |          384.5 |          384.9 |
| idle-storm-thumb-16n |          384.9 |          385.1 |
| idle-storm-thumb-8   |          384.2 |          385.5 |
| idle-upload          |          388.8 |          392.4 |
| probe                |          389.9 |          390.0 |
| scan-storm-edit-16n  |          401.2 |          404.4 |
| scan-storm-edit-8    |          391.5 |          393.7 |
| scan-storm-read-16n  |          432.6 |          570.7 |
| scan-storm-read-8    |          402.2 |          431.3 |
| scan-storm-thumb-16n |          402.3 |          399.9 |
| scan-storm-thumb-8   |          389.1 |          390.7 |
| scan-upload          |          713.0 |          710.5 |

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
| idle-storm-edit-16n  |            1.1 |            0.7 |
| idle-storm-edit-8    |            1.1 |            0.8 |
| idle-storm-read-16n  |            1.1 |            1.2 |
| idle-storm-read-8    |            0.7 |            0.7 |
| idle-storm-thumb-16n |            0.5 |            1.0 |
| idle-storm-thumb-8   |            0.6 |            0.9 |
| idle-upload          |            0.5 |            1.2 |
| probe                |            0.7 |            1.2 |
| scan-storm-edit-16n  |            0.6 |            1.4 |
| scan-storm-edit-8    |            0.7 |            0.8 |
| scan-storm-read-16n  |            0.6 |            1.3 |
| scan-storm-read-8    |            0.6 |            0.8 |
| scan-storm-thumb-16n |            0.5 |            0.7 |
| scan-storm-thumb-8   |            0.6 |            0.7 |
| scan-upload          |            0.6 |            1.2 |

| profile              | temp (100) | temp (200) |
| :------------------- | ---------: | ---------: |
| idle-storm-edit-16n  |        1.1 |        1.1 |
| idle-storm-edit-8    |        1.1 |        1.1 |
| idle-storm-read-16n  |        2.4 |       10.4 |
| idle-storm-read-8    |        1.1 |        1.1 |
| idle-storm-thumb-16n |        1.1 |        1.1 |
| idle-storm-thumb-8   |        1.1 |        1.1 |
| idle-upload          |        1.1 |        2.1 |
| probe                |        0.6 |        0.6 |
| scan-storm-edit-16n  |        1.1 |        1.1 |
| scan-storm-edit-8    |        1.1 |        1.1 |
| scan-storm-read-16n  |       35.7 |      142.6 |
| scan-storm-read-8    |       17.8 |       35.7 |
| scan-storm-thumb-16n |        1.1 |        1.1 |
| scan-storm-thumb-8   |        1.1 |        1.1 |
| scan-upload          |        0.6 |        2.2 |

| profile              | cache (100) | cache (200) |
| :------------------- | ----------: | ----------: |
| idle-storm-edit-16n  |         1.1 |         1.1 |
| idle-storm-edit-8    |         1.1 |         1.1 |
| idle-storm-read-16n  |         0.1 |         0.2 |
| idle-storm-read-8    |         1.1 |         1.1 |
| idle-storm-thumb-16n |         1.1 |         1.1 |
| idle-storm-thumb-8   |         1.1 |         1.1 |
| idle-upload          |         0.6 |         0.6 |
| probe                |         0.6 |         0.6 |
| scan-storm-edit-16n  |         1.1 |         1.1 |
| scan-storm-edit-8    |         1.1 |         1.1 |
| scan-storm-read-16n  |         0.6 |         1.5 |
| scan-storm-read-8    |         0.6 |         0.9 |
| scan-storm-thumb-16n |         1.1 |         1.1 |
| scan-storm-thumb-8   |         1.1 |         1.1 |
| scan-upload          |         0.6 |         0.6 |
