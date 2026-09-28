---
status: done
type: bug
priority: low
area: testing
---

API scenario harness defects found while writing Iteration 5 of
`.plan/test-exif-xmp-handling.md`. None block current coverage; each one
either hides an assertion or forces a workaround.

- **Fixed 2026-09-28 (Gap 6a).** `serve_image_ok` was a dead assertion:
  `check_file_and_serve_assertions` had no branch for it. Now implemented
  (compressed-route fetch, 200 + `image/jpeg` + JPEG magic bytes, harness-minted
  hash token); the scenario passed once wired — serving was never broken — and
  a product mutation makes it fail.
- **Fixed 2026-09-28 (Gap 6a).** `asset_id_as` on a `photo` given item was silently ignored unless `id_as`
  is also present: the given loop set `has_id_as` from `id_as` only, so the
  discovery pass never ran and `${var}` interpolated to empty. Fixed; pinned by
  `asset_id_as_without_id_as_binds_the_asset_id.yaml` (which had to bind
  nothing else — a sibling `id_as` masked the first draft).
- **Fixed 2026-09-28 (Gap 6a).** `wait_for_album_index` panicked on a `failed` index state instead of returning
  it, so "every matched file undecodable" outcomes could not be asserted.
  Now `wait_index: true | false | "completed" | "failed" | {expect: …}`;
  end-to-end pinned by `album_index_failed_when_every_matched_file_fails.yaml`
  (which needed `truncate_file` as a `when` verb — a `given`-only transform
  runs before the given phase's own scan and cannot produce a failed match).
- **Fixed 2026-09-28 (Gap 6c).** `backend/tests/schema.json` documented the
  scenario vocabulary but nothing loaded it. Now wired: `jsonschema` 0.58 as a
  backend dev-dependency (defaults off, deny-verified), `scenario_schema.rs`
  validates every scenario and selftest against it plus a valid-control /
  8-rejection suite. The wiring audit found 186 failures across 162 files —
  all six classes were schema bugs (incl. a `oneOf` ambiguity that made the
  status-code range unenforceable), zero scenario edits needed; the schema now
  also rejects bare `response.<path>` forms the interpreter would drop
  silently.
- **Fixed 2026-09-28 (Gap 6b).** The Playwright backend port race: `paths.ts::createPaths()` drew
  `30000 + random(30000)` when `WORKER_NUM` is unset, with no collision check,
  so two workers starting backends concurrently can hit `binding failed:
Address already in use` and one scenario fails on a port that was never
  free. Observed once across full-suite runs (the run passed on retry).
  Now: a bind-test probe redraws occupied ports (both the random and
  WORKER_NUM paths; probe narrows but does not close the window — stated in
  code), 13 unit tests in `frontend/tests/paths.test.ts`, and a decisive
  A/B where a held port failed pre-fix and passed post-fix. Orphan leaks
  measured per path: `process.exit` and startup-timeout now reap via a
  once-installed live-backends backstop; SIGKILL of the worker remains
  unfixable from Node (the probe is the compensating control). Also fixed
  the silent-corruption path found on the way: a backend whose bind fails
  stays alive with no listener, and `waitForServer` could resolve against
  _another_ backend's HTTP answer — it now rejects on `Address already in
use` in the child's stderr.

- **Fixed 2026-09-28 (Gap 6a).** Body assertions in a **non-last** `call:`'s `then:` block were silently
  ignored: `backend_api.rs` runs only `check_status_assertions` for inline
  `then`, so `response.json.*` / `array_where` there never execute. Found
  during `test-exif-xmp-handling` Iteration 5's successor plan when a worker's
  first draft of two scenarios passed vacuously (and a union mutation escaped
  both). Now every call's inline `then:` executes (status + body + file/serve),
  non-list `then` is a hard error, and `docs/scenario-dsl.md` documents it.
  The full audit enabled 15 never-run assertions across 13 scenarios: 13
  correct-and-passing, 2 flawed YAML (missing sync point after `write_file`;
  `locateTo` is a snapshot position, not a constant) — no hidden product
  failure. Pinned by `selftest/non_final_call_body_assertion_catches_wrong_value.yaml`.

- **Fixed 2026-09-28 (Gap 6c).** The `snapfab` CLI's library/random path
  sampled from `manifest.formats` (every declared format) instead of the
  randomizable set, panicking with `manifest format 'webp' is not generatable
by snapfab` on any run that drew a pinned format. It now draws from
  `randomizable_formats()` and honours each format's `FixturePlan` (generate
  vs copy-pinned-fixture — measured that sampling alone would not have fixed
  it, since all six formats are randomizable); 3 red tests plus a coverage
  test that kills the tempting partial fix. The seeded scenario path was
  always unaffected.

- **Fixed 2026-09-28 (Gap 6a).** `file_absent`/`file_exists` accepted a `${data_path}`-prefixed (absolute) path:
  the path is joined onto `image_home` after stripping the leading `/`, so it
  resolves under `<image_home>/<absolute path>/…`, can never exist, and the
  assertion passes regardless of the handler under test. Found 2026-09-28 by
  the Gap 5 upload work when a mutation failed to kill a scenario written that
  way. Now guarded (`image_home_path` rejects absolute results with a
  diagnostic), the two affected scenarios corrected, and
  `selftest/file_absent_rejects_absolute_path.yaml` pins the guard.

- **Fixed 2026-09-28 (Gap 6c).** The documented `response.<path> absent`
  assertion form had no implementation branch in `backend_api.rs` (the
  `"absent"` at the `array_where` handler is a different feature), so
  `response.json.foo: absent` asserted nothing useful. Now implemented:
  `resolve_json` + `assert_json_absent` pass iff the path does not resolve,
  with distinct messages for a present value and a present explicit `null`;
  two selftests carry the failing half; the mp4 video pin uses `absent` and
  now kills an `Option`-style `None` field that the old `null` pin missed.

## Notes

2026-09-26 — Recorded from the Iteration 5 report of test-exif-xmp-handling;
each item has its measurement in that plan's Progress section.
2026-09-28 — Added the vacuous `file_absent`/`file_exists` path defect from
the Gap 5 upload-diagnosis work.
2026-09-27 — Added the silent non-last-`then` body-assertion rule from the
exiftool-metadata-engine Iteration 2 report.

2026-09-28 — Gap 6c fixed the last three items; task closed. Also repaired
`selftest/json_assert_catches_wrong_value.yaml`, whose `response.json.length`
never resolved (it panicked on a Null mismatch for any expected value, so it
never demonstrated catching a wrong value) — now asserts a resolved
`response.json.[0].album_name`, which incidentally confirmed the schema
guards the `response.json.[0]` root-index form.
