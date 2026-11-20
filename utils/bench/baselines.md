# Benchmark baselines

Generated from `baselines.tsv` and the battery `report.tsv` files. `battery check` gates the baselined medians; context values (idle worst cases, scroll, bursts, memory footprint) come from the latest report.

## Terms

- **#images**: number of images in the scanned set.
- **scenario**: the server load a run applies: control (no extra load), a synthetic burst, or upload.
- **idle**: server at rest after the scan; the idle accesses are the baseline.
- **scan**: while the initial indexing scan of the image set runs.
- **upload**: while files are continuously uploaded and scanned.
- **burst**: a synthetic client loop repeating one access on top of the idle or scan state.
- **access**: the API action measured: read (grid prefetch), thumbnail (download), edit (metadata), scroll bar/rows/data (scroll requests), upload submit/visible (upload endpoints).
- **median**: median over repeat runs.
- **worst_case**: worst observed repeat value.
- **wall time**: elapsed wall-clock time of the scan.
- **tree build**: album tree rebuild stage inside the scan.
- **filter clone**: filter snapshot clone stage inside the scan.
- **peak memory**: peak resident memory of the server process.
- **DB**: on-disk database; index, cache, and temp are separate stores.
- **compressed store**: on-disk store of generated image files.
- **units**: ms = milliseconds, us = microseconds, s = seconds, MB = 10^6 bytes, MiB = 2^20 bytes, GiB = 2^30 bytes.

## Run environment

Host environment of the recorded baselines, captured once per battery run; `unknown` marks fields the collection predates.

- **host**: unknown (unknown)
- **CPU**: unknown, unknown cores, unknown/unknown MHz, frequency governor unknown, turbo boost unknown
- **memory**: unknown
- **load average (1/5/15 min)**: unknown/unknown/unknown at start, unknown/unknown/unknown at end
- **git**: fix/album-concurrency @ 590d41b992ee
- **captured**: 2026-11-20T21:51:28.282523+00:00 to unknown

## Memory footprint

Peak process memory and on-disk store sizes after the run.

| #images | scenario          | peak memory MiB | temp DB MB | index DB MB | cache DB MB | compressed store MB |
| ------: | ----------------- | --------------: | ---------: | ----------: | ----------: | ------------------: |
|      50 | control           |           370.0 |        1.1 |         0.3 |         0.6 |                 2.2 |
|     100 | control           |           373.5 |        1.1 |         0.5 |         0.6 |                 4.0 |
|     100 | read burst        |           369.3 |        0.6 |         0.6 |         0.6 |                 4.0 |
|     100 | thumbnail burst   |           370.0 |        1.1 |         0.6 |         0.6 |                 4.0 |
|     100 | edit burst        |           370.7 |        1.1 |         0.7 |         0.6 |                 4.0 |
|     100 | read burst (idle) |           366.2 |        0.6 |         0.5 |         1.1 |                 3.9 |
|     100 | upload            |           392.5 |       10.4 |         0.9 |         2.2 |                 4.0 |
|     200 | control           |           385.4 |        4.2 |         0.8 |         0.6 |                 7.8 |

## Idle access latency

Server at rest after the scan; these accesses are the baseline.

| #images | access      | median ms | worst_case ms |
| ------: | ----------- | --------: | ------------: |
|      50 | read        |      10.7 |          10.9 |
|      50 | thumbnail   |      11.3 |          13.0 |
|      50 | edit        |      12.1 |          13.4 |
|      50 | scroll bar  |       0.7 |           1.6 |
|      50 | scroll rows |       0.7 |           5.0 |
|      50 | scroll data |       1.1 |           7.7 |
|     100 | read        |      10.9 |          11.8 |
|     100 | thumbnail   |      10.2 |          12.6 |
|     100 | edit        |      12.2 |          15.6 |
|     100 | scroll bar  |       0.7 |           1.6 |
|     100 | scroll rows |       0.7 |           3.1 |
|     100 | scroll data |       1.3 |           8.6 |
|     200 | read        |      11.3 |          11.8 |
|     200 | thumbnail   |      12.6 |          13.4 |
|     200 | edit        |      13.0 |          13.3 |
|     200 | scroll bar  |       0.7 |           1.6 |
|     200 | scroll rows |       0.7 |           4.3 |
|     200 | scroll data |       1.8 |          10.9 |

## Scan access latency

While the initial indexing scan runs.

| #images | access    | median ms | worst_case ms |
| ------: | --------- | --------: | ------------: |
|      50 | read      |       7.9 |          12.0 |
|      50 | thumbnail |       3.2 |           7.5 |
|      50 | edit      |      10.0 |          16.7 |
|     100 | read      |       8.0 |          20.7 |
|     100 | thumbnail |       4.0 |          12.5 |
|     100 | edit      |       9.1 |          13.2 |
|     200 | read      |       8.0 |          14.8 |
|     200 | thumbnail |       3.0 |           9.2 |
|     200 | edit      |       9.2 |          14.1 |

## Upload access latency

While files are continuously uploaded and scanned.

| #images | access         | median ms | worst_case ms |
| ------: | -------------- | --------: | ------------: |
|     100 | read           |      12.5 |          16.9 |
|     100 | thumbnail      |       2.5 |           2.9 |
|     100 | edit           |      14.9 |          19.2 |
|     100 | upload submit  |       4.0 |           n/a |
|     100 | upload visible |     640.0 |           n/a |

## Burst access latency

A synthetic client burst of one access type on top of the idle or scan state.

| #images | scenario               | access    | median ms | worst_case ms | failed requests |
| ------: | ---------------------- | --------- | --------: | ------------: | --------------: |
|     100 | scan + read burst      | read      |       2.0 |          19.0 |               0 |
|     100 | scan + thumbnail burst | thumbnail |       4.0 |          20.0 |               0 |
|     100 | scan + edit burst      | edit      |       5.5 |          33.0 |             240 |
|     100 | idle + read burst      | read      |       2.0 |          16.0 |               0 |

## Scan wall time and stages

Wall time and the two dominant internal scan stages.

| #images | scenario          | wall time s (min-max)   | tree build median us | tree build worst_case us | filter clone median us | filter clone worst_case us |
| ------: | ----------------- | ----------------------- | -------------------: | -----------------------: | ---------------------: | -------------------------: |
|      50 | control           | 5.2585 (5.1485-5.3565)  |                459.0 |                    704.0 |                  541.5 |                      855.0 |
|     100 | control           | 9.4965 (9.3825-9.4975)  |                850.0 |                   1344.0 |                 1069.5 |                     1602.5 |
|     100 | read burst        | 9.3865 (9.3815-9.4895)  |                732.5 |                    750.0 |                  716.5 |                      783.0 |
|     100 | thumbnail burst   | 9.386 (9.3845-9.492)    |                660.0 |                    694.5 |                  686.0 |                      720.5 |
|     100 | edit burst        | 6.358 (6.357-7.464)     |                496.0 |                    567.5 |                  626.5 |                      699.0 |
|     100 | read burst (idle) | 9.684 (9.679-9.7)       |                  0.0 |                      0.0 |                    0.0 |                        0.0 |
|     100 | upload            | 17.656 (17.5585-17.658) |               1916.0 |                   2715.0 |                 2488.5 |                     3303.0 |
|     200 | control           | 18.069 (18.068-18.17)   |               1551.0 |                   2950.0 |                 1998.5 |                     3419.0 |
