---
status: backlog
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
- `backend/tests/schema.json` documents the scenario vocabulary but nothing
  loads it. Either wire it to a test (needs a JSON-Schema dependency) or drop
  it; its description now says it is unenforced.
- The Playwright backend port race: `paths.ts::createPaths()` draws
  `30000 + random(30000)` when `WORKER_NUM` is unset, with no collision check,
  so two workers starting backends concurrently can hit `binding failed:
Address already in use` and one scenario fails on a port that was never
  free. Observed once across full-suite runs (the run passed on retry);
  failed runs also leave an orphaned `picasu` process behind. Probe the port
  before binding, retry on EADDRINUSE, or derive the port from the run id.

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

- The `snapfab` CLI's library/random path samples from `manifest.formats`
  (every declared format) instead of `selection::randomizable_formats()`, so
  it panics with `manifest format 'webp' is not generatable by snapfab` on any
  run that draws a pinned format. Pre-existing at `94b8fc0d`; the seeded
  scenario path is unaffected (it uses the selector correctly).

- **Fixed 2026-09-28 (Gap 6a).** `file_absent`/`file_exists` accepted a `${data_path}`-prefixed (absolute) path:
  the path is joined onto `image_home` after stripping the leading `/`, so it
  resolves under `<image_home>/<absolute path>/…`, can never exist, and the
  assertion passes regardless of the handler under test. Found 2026-09-28 by
  the Gap 5 upload work when a mutation failed to kill a scenario written that
  way. Now guarded (`image_home_path` rejects absolute results with a
  diagnostic), the two affected scenarios corrected, and
  `selftest/file_absent_rejects_absolute_path.yaml` pins the guard.

## Notes

2026-09-26 — Recorded from the Iteration 5 report of test-exif-xmp-handling;
each item has its measurement in that plan's Progress section.
2026-09-28 — Added the vacuous `file_absent`/`file_exists` path defect from
the Gap 5 upload-diagnosis work.
2026-09-27 — Added the silent non-last-`then` body-assertion rule from the
exiftool-metadata-engine Iteration 2 report.
