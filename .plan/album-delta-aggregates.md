---
status: open
type: feature
priority: medium
area: backend
---

## Notes

Replace the album aggregate machinery (album_task: phase-1 read / phase-2
rayon recompute over the whole-tree snapshot / phase-3 full-record
insert; PENDING_ALBUM_UPDATES; the cascade album stage) with
**delta-maintained album aggregates**: every membership-mutating writer
op applies the album's aggregate delta in the same transaction as the
member row, as a field-level patch.

Why: the recompute existed only because album aggregates were not
transactionally reachable — no membership index, aggregates rebuilt by
brute force (O(P) snapshot scan per pending album, off-txn on rayon).
Delta maintenance makes stats transactionally consistent with membership
at every commit (O(1) per mutation), deletes the staleness concept, and
dissolves N10 at the root: under serialized write txns, field-level
patches commute — a delta patch (count/size only) and an edit_share RMW
(share_list only) read each other's committed fields and compose; there
is no concurrent full-record writer left. Confirmed 2026-11-14 (user):
"strongest solution"; the requirement that every modification event
performs its correct delta holds elsewhere in the design (FS-first,
same-txn invariants) — the key is clean code plus validation.

State-role context: album aggregates are DERIVED state (G5); share_list
is the DB-only exception — the field-level patch discipline is what
keeps the exception safe while derived fields change around it.

## Design

- New table `ASSET_BY_ALBUM` (`TableDefinition<&str, &str>`), one row per
  member: key `"<album_id>/<asset_id>"` → `asset_id`. Per-member rows,
  NOT a per-album list value: a list makes every add/remove an O(M)
  read-modify-write of the whole entry, defeating the O(1) delta. Add =
  insert one row, remove = delete one row (O(log)), enumerate an album =
  prefix range scan O(log + M), boundary recompute = iterate the entry.
  Naming symmetric with ASSET_BY_PATH / ASSET_BY_ID.
- Keyed by `album_id`, not `album_dir`: album_id is stable identity
  (rename-invariant, already carried by every member record); the dir is
  derivable from the album's AssetRecord and would force re-keying on
  every dir move.
- Delta fields: `item_count` ±1, `item_size` ±size, `last_modified_time`
  touch. Time-range (start_time/end_time) and cover: pure delta on
  additions; on removal of a boundary holder (the member that WAS the
  min/max timestamp or the cover pick), recompute that field from the
  member table in the same txn — O(M), rare, exact. No per-album ordered
  structure needed.
- Empty-album transition (direct members hit zero): keep the current
  product behavior — cover from a descendant's members if any
  (O(descendant members) on that transition only).
- Deleted-album detach: the detach path deletes the album's member rows
  and applies member deltas (it already resolves each record).
- Cascade: the album stage is deleted — `PENDING_ALBUM_UPDATES`,
  `album_task`, its `run_cpu` path and the shared per-entry snapshot go;
  `cascade_after_write` keeps expiry bookkeeping. Member writes mark
  nothing pending; there is nothing to defer.
- `edit_share`/`edit_album` unchanged: RMW field patches that compose
  with deltas per the commutation property above.
- The path-based / album-field membership filter question disappears:
  membership truth is the member table, maintained in the same txn as
  the member row — atomic with it, no drift window.

## Decisions

- 2026-11-14 (user): delta maintenance is the design; per-member rows
  over a list value; keyed by album_id over album_dir. N10 dissolves via
  patch commutation. All membership-mutating sites must apply their
  delta in the same txn — the standing requirement, validated by tests
  and the model.
- 2026-11-14: the interim snapshot/recompute design (field-level patch
  in album_task phase 3) is superseded by this task before it was ever
  implemented; the survey facts that led there (album_task phases,
  edit_share RMW, membership consumers) carry over.

## Steps

1. [done] Surveys: album_task phases (album.rs:46-110),
   edit_share RMW (edit_share.rs:137-155), membership consumers (tree
   view filter, list_album_assets_under table scan, sweep prefix scan
   asset_store.rs:38-56).
2. [open] Schema + store: `ASSET_BY_ALBUM` table; member-table helpers
   (add/remove/list/drop-album) in asset_store.rs; clear in reset
   (OP-11's clear_all_asset_tables_drop gains the new table).
3. [open] Delta sites (same-txn, field-level patch discipline — read the
   album record in-txn, patch aggregate fields, keep share_list/cover
   unless computed):
   - flush_tables (scan apply): member add; size change on re-index
     (read the old row in-txn for the delta); is_trashed changes
     membership (trash toggle site — verify where edit_flags commits).
   - sweep_remove_batch: per removed member, delta + member-row delete.
   - assign/move (OP-4/OP-5): delta on source album (−) and target
     album (+); dir moves (apply_dir_move_plans) re-key member rows for
     renamed/rewritten albums in the same txn.
   - deleted-album detach: member-row delete + deltas.
4. [open] Boundary strategy: exact time-range/cover on addition; on
   boundary-holder removal, recompute from the member table in-txn;
   empty-album descendant-cover transition.
5. [open] Delete the cascade album stage; verify no remaining callers of
   album_task / mark_album_for_update / drain_pending_album_updates.
6. [open] Catalogs: OP-8 (ALBUM_STATS) rewritten — the marker moves to
   the delta application in flush_tables (or a named helper); operations
   properties re-linked; REQ-16 (delta-maintained album aggregates)
   derived from G5/G1; the old N10/REQ-15-era album_task framing
   updated. Model-to-code terminology table updated.
7. [open] Backend tests: per-site delta tests (add/remove/size
   change/trash/move both sides/dir-move re-key/batch remove), boundary
   recompute, empty transition, patch commutation (delta txn + edit_share
   txn interleave → both survive), drift guard (a membership mutation
   without its delta is a bug — test the enumerated sites).
8. [open] Model rework: `AlbumTaskApply`/cascade actions replaced by
   delta actions on the album aggregate fields (separate commit for the
   action rename per the terminology rule); `AlbumStatsConsistentWhenDrained`
   strengthened to always-consistent; the staleness runs replaced by
   delta runs (add/remove/boundary/empty-transition); N10 race-absorption
   run replaced by the patch-commutation run; N13 node rewritten (stats
   consistent by construction). Traceability regenerated.
9. [open] Reset/restore paths: OP-11 clear includes ASSET_BY_ALBUM;
   verify the restore/backstop flow re-seeds member rows from
   pipeline-maintained membership.

## Open questions

- Root albums: files directly at a root (IMAGE_HOME, USERS_HOME, future
  roots) are currently unalbummed (ensure_dir_albums returns None for
  root-level files). If root-as-album browsing is wanted, that is an
  ensure_dir_albums change plus a root dir-album per root; the delta
  machinery covers it identically. Product decision, not made here.
- Value payload of ASSET_BY_ALBUM rows: plain asset_id (recompute reads
  records via ASSET_BY_ID) vs cached size/timestamp (O(M) recompute
  without record reads, but a cache-obligation on size changes). Start
  plain; add caching only if the boundary recompute shows up in
  profiling.
- Backstop recompute hook: optional debug/repair comparison of
  item_count against the member-table count, reusing the backstop scan
  cadence (ASS-7). Parked until a drift report exists.

Reference: G5, ASS-3, node N10, .plan/quint-asset-aspects.md step 14c,
utils/quint/README.md (Source of truth, terminology). REQ-15-era survey
facts carry over.
