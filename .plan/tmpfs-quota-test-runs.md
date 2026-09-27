---
status: backlog
type: chore
priority: medium
area: testing
---

The shared `/tmp` (32 G tmpfs) fills up during test runs — Playwright harness
temp dirs (`/tmp/opencode/picasu-*/.testruns`, `playwright-*`), cargo/cc
intermediates, and concurrent worktrees all live there. Two workers hit
`Disk quota exceeded` mid-run during the test-exif-xmp-handling plan
(iterations 4 and 6), which produced spurious build failures and crashed
Chromium pages.

Worse, `git stash pop` under quota pressure restored untracked files as
0-byte versions of themselves (recovered from the stash, digests verified).

Mitigations that work today, used ad hoc:

- `TMPDIR=/home/codesam/.cache/picasu-scratch-tmp` (cc writes intermediates
  to `$TMPDIR`) and, when needed,
  `CARGO_TARGET_DIR=/home/codesam/.cache/picasu-target`.
- Prune stale `.testruns`/`playwright-*` dirs and
  `target/debug/incremental` before long runs.

Wanted: a repository mechanism — e.g. a `just` recipe that reports/cleans
stale test-run dirs, pointing harness temp output outside `/tmp`, or
documenting the env overrides in `docs/test-strategy.md` — so the workaround
is not re-discovered per worker.

## Notes

2026-09-27 — Recorded from Iteration 6 of `.plan/test-exif-xmp-handling.md`.
