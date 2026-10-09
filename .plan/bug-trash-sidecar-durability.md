---
status: done
type: bug
priority: high
area: backend
---

## Problem (verified 2026-10-09)

Soft-delete state is durable only in RedB. `PUT /put/edit_flags` stores
`is_trashed` on `AssetRecord` (`router/put/edit_flags.rs:100`), and
`write_sidecar_for` (`process/xmp_write.rs:55`) manages only tags,
description, rating and album title — nothing carries the trash flag to disk.
Trash's semantic is that the file _stays_ on disk, so a rebuild from the
filesystem (`POST /post/rebuild`, docs/database.md: "rows are rebuilt rather
than merged") composes every trashed asset as a normal one
(`abstract_data.rs:522` defaults `is_trashed: false`). Losing the DB
undeletes the whole trash view silently, which violates the design doc's own
invariant that generated state "must be rebuildable from the filesystem"
(docs/design.md:35,42).

## Design (decided 2026-10-09)

**A managed XMP property carries the trash flag, per sidecar.** A duplicate
of the sidecar-as-source-of-truth pattern the four existing managed fields
already follow (`process/sidecar_edit.rs` module doc: file+sidecar is the
source of truth, METADATA_TABLE is a cache of it).

- Namespace `http://picasu.app/xmp/1.0/`, prefix `picasu`, property
  `Trashed` as `XmpValue::Boolean`. xmpkit 0.1.6 verified: `parse` adopts
  packet-declared namespaces; writes need one global
  `xmpkit::register_namespace` (per-thread registry, so registered before
  each set/delete, not once per process).
- `XmpData.trashed` + normalize reads it (`process/xmp.rs`); compound-value
  tolerance: `Boolean(true)` or a truthy `String` ("true"/"1"), so a
  hand-written packet still reads.
- Write semantics mirror the existing managed fields: `true` sets the
  property, `false` deletes it (restore must clear it, the way
  `custom_title: None` removes `dc:title`).
- Write path goes through `commit_metadata_edits` (sidecar first, rollback on
  failure, store phase after) — same contract as edit_tag/edit_rating.
- Read/index path: `process/index.rs` (image+video) and
  `process/dir_album.rs` `write_album_to_db` compose the flag from
  `XmpData.trashed`, so an incremental index and a full rebuild both restore
  it. Permanent delete already removes file+sidecar together
  (`router/delete.rs`), so state dies with the file.

**Conflict policy (decision, no backfill):** during normal operation the
DB record is authoritative and the watcher never clears a flag when a
sidecar appears or is rewritten; on a rebuild the sidecar wins. Files
trashed before this change keep DB-only state: they revert to normal on DB
loss, a documented one-time window, and a backfill pass is deliberately out
of scope.

**Out of scope:** album trashing semantics (only the selected item is
flagged — unchanged), moving trashed files to an excluded directory, and
the durable-journal alternative.

## Scope

- `process/xmp.rs` — `XmpData.trashed`, normalize, namespace const/register
- `process/xmp_write.rs` — managed trash field in `apply_managed_fields`
- `model/abstract_data.rs` — `is_trashed()` / `set_trashed()`
- `process/index.rs`, `process/dir_album.rs` — compose flag at index time
- `router/put/edit_flags.rs` — route trash through `commit_metadata_edits`
- `docs/design.md` — two-phase delete + sidecar durability rule
- `backend/tests/scenarios/trash_state_survives_rebuild.yaml`

## Acceptance

1. Packet with `picasu:Trashed=true` (Boolean) reads back trashed; absent,
   `false`, or malformed reads back untrashed.
2. Trashed item's sidecar gains the property; restore removes it; foreign
   properties survive the read-modify-write; album sidecar
   (`.albuminfo.xmp`) behaves the same.
3. Scenario: trash a photo → sidecar exists → `POST /post/rebuild` → the
   asset is still trashed (`Trashed:true` query finds it, `Trashed:false`
   does not) → restore + rebuild → normal asset again.
4. Failed sidecar write fails the edit (no store phase), inherited from
   `commit_metadata_edits`; no new test needed beyond the module's pins.

## Progress (2026-10-09) — implemented, backend suite green

- `xmp.rs`: `XmpData.trashed`, `NS_PICASU`/`ensure_picasu_namespace_registered`,
  tolerant reader (`Boolean`, `true`/`True`/`1`, `Integer != 0`).
- `xmp_write.rs`: `apply_managed_fields` sets `picasu:Trashed` when trashed and
  deletes it on restore; `write_sidecar_for` sources the flag from the composed
  view.
- `abstract_data.rs`: `is_trashed()` / `set_trashed()`.
- `process/index.rs` (image + video): compose the flag from the sidecar at
  index time; `dir_album.rs`: `is_trashed: albuminfo.trashed` for dir-albums.
- `rebuild.rs`: media — `store_metadata_record(.., Some(data.is_trashed()))`
  (the walk's identity row could not know the flag before the sidecar was
  read); albums — the walk sets the flag from `read_albuminfo` (now
  `pub(crate)`).
- `edit_flags.rs`: routes trash through `commit_metadata_edits` — sidecar
  first with rollback, store phase after, precondition failure (401) and
  unfound index (skipped, 200) preserved.
- Scenario `backend/tests/scenarios/trash_state_survives_rebuild.yaml` pins
  the reported flow; red states observed during the change (no sidecar after
  edit_flags; `isTrashed: false` after rebuild).
- Full backend suite: 458 passed. `openapi.json` regenerated (the handler's
  doc comment is the published operation description).
- Residual (unchanged): list-level counting in the harness's tree snapshot
  sees stale rows after `POST /post/rebuild`, so the scenario pins state at
  the record level via `GET /get/metadata` rather than via listing counts.
  Files trashed before this change keep DB-only state and revert to normal on
  DB loss (documented one-time window, no backfill).
