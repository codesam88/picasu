# Parser fixtures

The parser fixtures are **recorded artifacts** — files copied out of a real
cell, or exact excerpts of one. A hand-written line that looks like server or
harness output is not a fixture: it tests the parser's idea of a format, not
the format the backend and workload actually write.

Why this rule exists: the S2 miss shipped fabricated `observe drain:` lines in
a `harness-a.log` transcript and the tests passed, while every real cell wrote
its stage tables to `server.log` and extracted an empty `stages.json`. The
tests proved the parser agreed with a log dialect nothing produced. Recorded
fixtures make that failure mode visible: the golden test reads the same bytes
a cell writes, and a parser change that stops matching them fails.

Every fixture is pinned by `tests/test_recorded_fixtures.py` to the parser
output it produces. Replacing a fixture means updating that golden to the new
recording, not editing either side to agree.

## Rule

- **Recorded**: copied byte-for-byte from a real cell, or an exact excerpt of
  one. Record the source cell, its date, and why it is an excerpt.
- **Derived**: a value-for-value replay of a recorded artifact, allowed only
  when the recorded cell cannot carry the file itself (the event stream
  postdates these cells). The source artifacts stay in the fixture tree and a
  test re-derives the file from them.
- **Synthetic**: allowed only for error shapes — an unknown schema version,
  binary garbage, a stream cut mid-line, a missing key. These stay inline in
  the test that needs them, marked as synthetic, and never stand in for a
  valid-path fixture.

## Files

| Fixture                                                 | Verdict  | Source                                                                                                                         |
| ------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `server-drain.log`                                      | recorded | `/tmp/picasu-bench-distort-on/server.log`, full file (110 lines, `PICASU_OBSERVER_STATS=1`, 2 scans), 2026-11-14 03:55–03:56 Z |
| `server-stats-off.log`                                  | recorded | `/tmp/picasu-bench-distort-off/server.log`, full file (112 lines, no stats banner, no drain tables), 2026-11-14 03:57–03:58 Z  |
| `events-dual/control/harness-a.log`                     | recorded | `/dev/shm/picasu-bench-gate2/gate-upload-rep2`, full transcript, 2026-11-15, git `0d9f3265`                                    |
| `events-dual/control/server.log`                        | excerpt  | same cell, recorded lines 94–148: the two `observe drain:` tables through shutdown; startup prefix omitted                     |
| `events-dual/control/status-final.json`                 | recorded | same cell, full file                                                                                                           |
| `events-dual/control/scan-walls.tsv`                    | recorded | same cell, full file                                                                                                           |
| `events-dual/control/run-manifest.json`                 | excerpt  | same cell, identity keys the extractor and battery read; machine/env fields omitted                                            |
| `events-dual/control/events.jsonl`                      | derived  | replay of the recorded transcript and `rounds-run-*/probe-round-1.tsv`; no cell of this date has a stream                      |
| `events-dual/control/rounds-run-1/probe-round-1.tsv`    | recorded | same cell, full file (10 probe latencies)                                                                                      |
| `events-dual/control/rounds-run-2/probe-round-1.tsv`    | recorded | same cell, full file                                                                                                           |
| `events-dual/storm-diff/harness-a.log`                  | recorded | `/dev/shm/picasu-bench-gate2/gate-storm-diff-rep1`, full transcript, 2026-11-15, git `0d9f3265`                                |
| `events-dual/storm-diff/status-final.json`              | recorded | same cell, full file                                                                                                           |
| `events-dual/storm-diff/scan-walls.tsv`                 | recorded | same cell, full file                                                                                                           |
| `events-dual/storm-diff/run-manifest.json`              | excerpt  | same cell, identity keys only                                                                                                  |
| `events-dual/storm-diff/rounds-run-1/storm-round-1.tsv` | recorded | same cell, full per-op storm samples (80 rows)                                                                                 |
| `events-dual/storm-diff/rounds-run-2/storm-round-2.tsv` | recorded | same cell, full per-op storm samples (80 rows)                                                                                 |
| `events-dual/storm-diff/events.jsonl`                   | derived  | replay of the two storm TSVs and the recorded summary/verify lines                                                             |

`drain-multi.log` was a byte-identical duplicate of `server-drain.log`; it was
removed, leaving one copy of the recording.

### The derived streams

`events.jsonl` is the only shape the recorded cells cannot supply: the stream
was added after those cells ran. Both files are replays, not reconstructions
from memory:

- `storm-diff`: each `storm_sample` is one row of the recorded
  `storm-round-*.tsv` in recorded order (`client`, `op`, latency in ms at the
  TSV's 6-decimal precision, `code`); each `storm_summary` is the
  median/p95/max/failed of that TSV; each `verify_result` is a recorded
  `storm verify: matched N/N` line.
- `control`: each `upload_file` is a recorded `upload file=...` line, each
  `scan_state` a recorded `scan run ...` line, each fetch `probe_round` the
  mean/min/max of the recorded `probe-round-1.tsv`. The hyperfine medians are
  the values the transcript prints; that round's export is not checked in.

Event timestamps (`t`) are replay placeholders: they mark order inside a run,
not the wall clock the producer would have written, and the extractor does not
read them. Replace both streams with a real cell's `events.jsonl` once such a
cell is recorded.

## Synthetic inputs

These live inline in `tests/test_extract_bench.py` and are synthetic by
design; they exercise failure reporting, not parsing of valid output:

- unknown schema version (`v=2`)
- a stream cut mid-line (`{"v":1,"type":"scan_st`)
- an unreadable stream (`b"\xff\xfe\x00garbage"`)
- an unknown event type, and an event missing a required key

## Recording a new fixture

1. Pick a cell under a battery parent (`/dev/shm/picasu-bench-*`), copy the
   artifact with `cp` (no hand edits), or quote the exact line range and note
   it here.
2. Update or add the golden in `tests/test_recorded_fixtures.py` from the
   parser's output for the new bytes.
3. Add the row to the table above: path, verdict, source cell and date, and
   the excerpt reason.
