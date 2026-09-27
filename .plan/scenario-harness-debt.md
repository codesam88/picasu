---
status: backlog
type: bug
priority: low
area: testing
---

API scenario harness defects found while writing Iteration 5 of
`.plan/test-exif-xmp-handling.md`. None block current coverage; each one
either hides an assertion or forces a workaround.

- `serve_image_ok` is a dead assertion:
  `backend/src/tests/backend_api.rs::check_file_and_serve_assertions` has no
  branch for it, so `image_serving_survives_album_move_v.yaml` asserts nothing
  on its final step. Implementing the branch may turn the suite red for reasons
  unrelated to whatever change introduced it — run the full scenario suite
  after wiring it.
- `asset_id_as` on a `photo` given item is silently ignored unless `id_as`
  is also present: the given loop sets `has_id_as` from `id_as` only, so the
  discovery pass never runs and `${var}` interpolates to empty. Every scenario
  currently works around it by setting both keys.
- `wait_for_album_index` panics on a `failed` index state instead of returning
  it, so "every matched file undecodable" outcomes cannot be asserted in a
  scenario (the harness itself crashes). Decide whether `failed` should become
  an assertable state or whether undecodable-but-signature-matching files
  should be skipped like unrecognized ones.
- `backend/tests/schema.json` documents the scenario vocabulary but nothing
  loads it. Either wire it to a test (needs a JSON-Schema dependency) or drop
  it; its description now says it is unenforced.

## Notes

2026-09-26 — Recorded from the Iteration 5 report of test-exif-xmp-handling;
each item has its measurement in that plan's Progress section.
