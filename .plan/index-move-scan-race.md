---
status: backlog
type: bug
priority: high
area: backend
---

## Problem

Indexing runs in the background (WalkDir lazy + tokio::spawn per file in
`tasks/actor/album_index.rs:230-279`) while files can be moved/renamed/deleted
by the user (assign_album) or by the watcher. A file present at walk time may
be gone by the time the pipeline reads it, producing:

`ERROR picasu::error Failed to run index task ... failed to process image
metadata pipeline Hash:..., Path: sandbox/images/.../PXL_0039.png → failed to
decode image into DynamicImage → failed to read file into memory → ENOENT
(os error 2)`

Symptom observed as grey frames when moving during a scan; moving after the
scan finishes is fine.

## Evidence trace

- `tasks/actor/album_index.rs:231-279` — lazy WalkDir + per-file spawn; no
  existence re-check before processing.
- `workflow/mod.rs:118-177` — `index_image`: OpenFileTask → HashTask →
  path-dedupe guard (`:150-156`, same-path only) → DeduplicateTask →
  IndexTask.
- `tasks/actor/open_file.rs:23-63` — open retries, but treats ENOENT like any
  open failure.
- `tasks/actor/hash.rs:18-30` — hashes already-opened File handle.
- `tasks/actor/index.rs:26-35,51-103:71` — IndexTask reads file again via
  `process_media_info`; on failure inserts a `pending` placeholder row
  (`:86-88`) and returns Err.
- `process/index.rs:36-83:62` → `process/misc.rs:60-88:74` —
  `generate_dynamic_image` → `std::fs::read` with no retry/tolerance.
- `router/put/assign_album.rs:119-180,186-274:244` — `fs::rename` of file +
  sidecar; no check against index IN_PROGRESS; no requeue of in-flight rows.
- `tasks/batcher/start_watcher.rs:184-215,261-267` — Remove event prunes the
  matching row (`handle_removed_file` / `prune_asset_path`).
- `tasks/actor/album_index.rs:270-278,604-621` — per-file errors do not abort
  the scan; `increment_failed`.
- `tasks/actor/album_index.rs:504-573` — sweep prunes stale asset paths after.

## Why high priority

Background indexing during file organization is normal operation, not a corner
case. The race fires routinely when users move photos while a scan is running,
leaving ERROR noise and grey placeholder rows.

## Scope

- Tolerant missing-file handling in the pipeline: treat ENOENT as expected
  skip + cleanup, not ERROR noise.
- Coordination between index IN_PROGRESS and assign_album/move: skip, defer,
  or requeue so a moved file is not indexed at its old path (and is picked up
  at the new path).
- Prefer reading by the already-open handle rather than re-reading by path in
  `process_media_info` if feasible.
- No frontend changes in this bug.

## Acceptance

- Controlled repro (rename/delete after hash but before IndexTask read, plus
  assign_album mid-scan) yields a clean skip and correct final state at both
  old and new paths.
- Full index still completes; no grey-frame leftover from pending/pruned rows.
- ENOENT during normal move no longer logs as ERROR.

## Notes

Leave implementation/status decisions to the architect. Do not implement the
fix in this task file.
