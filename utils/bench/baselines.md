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

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 2119/2400 MHz, frequency governor powersave, turbo boost disabled
- **memory**: 62 GiB
- **load average (1/5/15 min)**: 7.71484375/4.9912109375/2.7978515625 at start, 6.53271484375/6.294921875/4.68115234375 at end
- **git**: perf/new-baseline2 @ e3b386c59e79
- **captured**: 2026-11-21T06:12:31+00:00 to 2026-11-21T06:22:35+00:00

- **host**: codex (7.0.0-38-generic)
- **CPU**: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 8 cores, 2200/2400 MHz, frequency governor powersave, turbo boost disabled
- **memory**: 62 GiB
- **load average (1/5/15 min)**: 2.1923828125/2.78125/1.8046875 at start, 7.71484375/4.9912109375/2.7978515625 at end
- **git**: perf/new-baseline2 @ e3b386c59e79
- **captured**: 2026-11-21T06:09:53+00:00 to 2026-11-21T06:12:30+00:00

## Memory footprint

Peak process memory and on-disk store sizes after the run.

| #images | scenario          | peak memory MiB | temp DB MB | index DB MB | cache DB MB | compressed store MB |
| ------: | ----------------- | --------------: | ---------: | ----------: | ----------: | ------------------: |
|      50 | control           |           369.7 |        1.1 |         0.4 |         0.6 |                 2.2 |
|     100 | control           |           374.4 |        1.1 |         0.5 |         0.6 |                 4.0 |
|     100 | read burst        |           369.4 |        0.6 |         0.5 |         0.6 |                 4.0 |
|     100 | thumbnail burst   |           370.0 |        1.1 |         0.6 |         0.6 |                 4.0 |
|     100 | edit burst        |           370.7 |        1.1 |         0.6 |         0.6 |                 4.0 |
|     100 | read burst (idle) |           366.3 |        0.6 |         0.6 |         1.1 |                 3.8 |
|     100 | upload            |           394.5 |       10.4 |         1.1 |         2.2 |                 4.0 |
|     200 | control           |           382.0 |        2.1 |         1.0 |         0.6 |                 7.8 |

## Idle access latency

Server at rest after the scan; these accesses are the baseline.

| #images | access      | median ms | worst_case ms |
| ------: | ----------- | --------: | ------------: |
|      50 | read        |      11.7 |          12.6 |
|      50 | thumbnail   |      10.3 |          16.7 |
|      50 | edit        |      13.9 |          14.7 |
|      50 | scroll bar  |       0.7 |           1.7 |
|      50 | scroll rows |       0.8 |           3.0 |
|      50 | scroll data |       1.5 |           7.5 |
|     100 | read        |       7.2 |           7.5 |
|     100 | thumbnail   |       7.6 |           7.9 |
|     100 | edit        |       8.5 |          10.6 |
|     100 | scroll bar  |       0.7 |           2.4 |
|     100 | scroll rows |       0.8 |           4.1 |
|     100 | scroll data |       1.9 |          11.3 |
|     200 | read        |      11.6 |          14.4 |
|     200 | thumbnail   |      12.9 |          16.9 |
|     200 | edit        |      13.4 |          16.1 |
|     200 | scroll bar  |       0.8 |           2.0 |
|     200 | scroll rows |       0.8 |           7.3 |
|     200 | scroll data |       2.2 |          12.0 |

## Scan access latency

While the initial indexing scan runs.

| #images | access    | median ms | worst_case ms |
| ------: | --------- | --------: | ------------: |
|      50 | read      |      10.1 |          14.9 |
|      50 | thumbnail |       5.6 |          13.6 |
|      50 | edit      |      12.6 |          13.8 |
|     100 | read      |      10.8 |          16.7 |
|     100 | thumbnail |       5.8 |          16.9 |
|     100 | edit      |      12.9 |          20.0 |
|     200 | read      |      10.0 |          23.0 |
|     200 | thumbnail |       6.0 |          17.6 |
|     200 | edit      |      11.9 |          17.0 |

## Upload access latency

While files are continuously uploaded and scanned.

| #images | access         | median ms | worst_case ms |
| ------: | -------------- | --------: | ------------: |
|     100 | read           |      10.4 |          11.7 |
|     100 | thumbnail      |       1.6 |           1.8 |
|     100 | edit           |      12.7 |          14.0 |
|     100 | upload submit  |       3.0 |           n/a |
|     100 | upload visible |     634.0 |           n/a |

## Burst access latency

A synthetic client burst of one access type on top of the idle or scan state.

| #images | scenario               | access    | median ms | worst_case ms | failed requests |
| ------: | ---------------------- | --------- | --------: | ------------: | --------------: |
|     100 | scan + read burst      | read      |       3.0 |          21.0 |               0 |
|     100 | scan + thumbnail burst | thumbnail |       3.5 |          45.0 |               0 |
|     100 | scan + edit burst      | edit      |       9.0 |          56.0 |             240 |
|     100 | idle + read burst      | read      |       2.0 |          21.0 |               0 |

## Scan wall time and stages

Wall time and the two dominant internal scan stages.

| #images | scenario          | wall time s (min-max)     | tree build median us | tree build worst_case us | filter clone median us | filter clone worst_case us |
| ------: | ----------------- | ------------------------- | -------------------: | -----------------------: | ---------------------: | -------------------------: |
|      50 | control           | 5.456 (5.2525-5.462)      |                454.0 |                    626.5 |                  601.5 |                      793.0 |
|     100 | control           | 10.1045 (9.6965-10.6055)  |                862.5 |                   1821.5 |                 1087.0 |                     2516.0 |
|     100 | read burst        | 9.6945 (9.489-10.909)     |                510.5 |                    538.0 |                  499.5 |                      568.5 |
|     100 | thumbnail burst   | 9.897 (9.699-10.102)      |                508.0 |                    555.0 |                  524.5 |                      572.0 |
|     100 | edit burst        | 6.5635 (6.464-7.075)      |                653.5 |                    688.5 |                  787.0 |                      822.5 |
|     100 | read burst (idle) | 9.894 (9.89-10.089)       |                  0.0 |                      0.0 |                    0.0 |                        0.0 |
|     100 | upload            | 18.676 (17.9725-18.772)   |               1906.5 |                   2569.0 |                 2476.5 |                     3394.5 |
|     200 | control           | 18.7895 (18.3815-19.7995) |               1547.5 |                   2912.5 |                 2026.5 |                     5267.0 |
