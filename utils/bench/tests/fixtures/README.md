# Parser fixtures

The parser fixtures are **recorded artifacts** — files copied out of a real
cell, or exact excerpts of one. A hand-written line that looks like server or
harness output is not a fixture: it tests the parser's idea of a format, not
the format the backend and workload actually write.

Why this rule exists: the S2 miss shipped fabricated `observe drain:` lines in a
`harness-a.log` transcript and the tests passed, while every real cell wrote
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
  when the recorded cell cannot carry the file itself. The source artifacts
  stay in the fixture tree and a test re-derives the file from them.
- **Synthetic**: allowed only for error shapes — an unknown schema version,
  binary garbage, a stream cut mid-line, a missing key. These stay inline in
  the test that needs them, marked as synthetic, and never stand in for a
  valid-path fixture.

## Contents

- `recorded/probe-50/` — cell `probe-50-rep1` of the matrix run
  (`/var/tmp/matrix-final4-20261122-191029`, 2026-11-22), copied
  byte-for-byte: `events.jsonl`, `harness-a.log`, `run-manifest.json` and
  `status-final.json`. `server.log` is an exact excerpt: the stats banner
  plus the two `observe drain:` tables and their counters lines (the rest
  of the log is unrelated request noise).
- `server-drain.log` — a stats-on record with two scan tables, copied from
  a recorded cell of the 2026-11-15 gate battery.
- `server-stats-off.log` — the same run shape with observer tables off,
  copied from the same battery.
