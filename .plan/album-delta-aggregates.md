---
status: done
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
  touch. Time-range (start_time/end_time) and cover unified rule: any
  delta that changes a member's size or timestamp recomputes time-range
  and cover from the member table in the same txn (O(M)); additions
  adjust min/max by the new member's timestamp and set the cover to the
  newest member by timestamp; removal of a boundary holder recomputes
  from the member table. Cover rule: cover = newest member by timestamp
  (matching `recompute_cover_from_members`); a delta-add compares the new
  member's timestamp against the current cover member's timestamp and
  replaces the cover when newer. No per-album ordered structure needed.
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
- 2026-11-15 (review): single-txn discipline fixes (F1-F5): removal,
  sweep, move, trash toggle, detach/sweep-dir-albums all perform record
  deletes, member-row changes, and album aggregate deltas in one txn.
  Unified range/cover rule (F6): any size/timestamp change recomputes
  time-range + cover from the member table. Cover rule unified to
  newest-by-timestamp. Error discipline (F7): every delta/member/commit
  failure propagates — the enclosing transaction aborts rather than
  committing partial state; background sites log the failure at the
  writer/batcher edge, request sites surface a 500. A failed delta aborts
  the txn; a 500 beats silent drift. Dead interactive path + doc cleanup
  (F8). Steps 7-9 progress.

## Functional invariants (derived 2026-11-15; test + model anchors)

- I1 aggregate exactness: `item_count == |members|`, `item_size == Σ
sizes`, always (not quiescence). Tests:
  `membership_mutations_leave_members_and_aggregates_exact`,
  `album_member_write_keeps_aggregates_exact`. Model: the strengthened
  `AlbumStatsConsistentWhenDrained` → unconditional.
- I2 delta atomicity: member row + delta commit atomically; an aborted
  delta leaves both unchanged. Model: same-step discipline (membership
  actions in asset-aspects.qnt).
- I3 patch commutation: delta patches and edit_share RMWs compose in any
  interleaving. Test: `album_delta_and_share_list_patches_commute`.
  Model: `aspectsAlbumDeltaThenShareEditTest` /
  `aspectsShareEditThenAlbumDeltaTest`.
- I4 range/cover under the unified rule (min/max timestamps, newest-member
  cover) after any member mutation, incl. boundary-holder removal.
- I5 re-application idempotence: re-flush of an unchanged asset never
  double-counts.
- I6 exclusions: unalbummed (root-level) and trashed assets contribute
  nothing; trash toggle moves aggregates exactly.
- I7 empty transition: descendant-cover rule on last member leaving;
  re-establish on rejoin.
- I8 heal composition: legacy torn residue is cleaned and the healing
  deltas converge (no double-count on re-encounter).
- I9 batch prefix-consistency: per-path atomic batch removals — a crash
  mid-batch leaves completed paths fully applied, aggregates exact.
- I10 error atomicity: a failed delta aborts the operation txn
  (propagation policy, 2026-11-15). Model: atomic by construction (no
  explicit modeling).

I1–I3, I10 are the model-port targets for step 8.

## Steps

1. [done] Surveys: album_task phases (album.rs:46-110),
   edit_share RMW (edit_share.rs:137-155), membership consumers (tree
   view filter, list_album_assets_under table scan, sweep prefix scan
   asset_store.rs:38-56).
2. [done] Schema + store: `ASSET_BY_ALBUM` table; txn-scoped member
   helpers and `apply_album_delta` in asset_store.rs; clear_all_asset_tables_drop
   gains the table.
3. [done] Delta sites: flush_tables (insert+remove), sweep_remove_batch,
   assign move (source - / target +), deleted-album detach (drop member rows +
   detach remaining members), empty-ancestor cover refresh.
4. [done] Boundary strategy: pure delta on addition; removal recomputes
   time range/cover from member table; empty-album descendant-cover transition.
5. [done] Cascade album stage deleted (album_task, run_cpu album call,
   drain_pending_album_updates, mark_album_for_update call sites removed).

6. [open] Catalogs: OP-8 (ALBUM_STATS) rewritten — the marker moves to
   the delta application in flush_tables (or a named helper); operations
   properties re-linked; REQ-16 (delta-maintained album aggregates)
   derived from G5/G1; the old N10/REQ-15-era album_task framing
   updated. Model-to-code terminology table updated.
7. [done] Backend tests: per-site delta tests (add/remove/size
   change/trash/move both sides/dir-move re-key/batch remove), boundary
   recompute, empty transition, patch commutation (delta txn + edit_share
   txn interleave → both survive), drift guard (a membership mutation
   without its delta is a bug — test the enumerated sites). Tests:
   storage::asset_store::tests::{album_member_rows_roundtrip_and_prefix_isolation,
   apply_album_delta_add_updates_count_size_range_and_cover,
   apply_album_delta_remove_recomputes_boundary_holders,
   apply_album_delta_size_update_recomputes_range_and_cover,
   apply_album_delta_empty_transition_uses_descendant_cover,
   apply_album_delta_count_never_underflows,
   album_delta_and_share_list_patches_commute,
   move_asset_mapping_moves_membership_between_albums};
   tasks::batcher::flush_tree::tests::{flush_album_insert_records_member_and_aggregates,
   flush_album_size_change_updates_aggregate,
   flush_remove_drops_member_and_aggregates,
   sweep_remove_batch_drops_member_and_aggregates};
   router::put::assign_album::tests::dir_move_preserves_album_membership_and_aggregates;
   tests::album_delta::{trash_toggle_adds_and_removes_member,
   deleted_album_detach_drops_member_rows,
   membership_mutations_leave_members_and_aggregates_exact};
   tasks::queue::writer::tests::album_member_write_keeps_aggregates_exact.
8. [done] Model rework: `AlbumTaskApply`/cascade actions replaced by
   delta actions on the album aggregate fields (separate commit for the
   action rename per the terminology rule); `AlbumStatsConsistentWhenDrained`
   strengthened to always-consistent; the staleness runs replaced by
   delta runs (add/remove/boundary/empty-transition); N10 race-absorption
   run replaced by the patch-commutation run; N13 node rewritten (stats
   consistent by construction). Traceability regenerated.
9. [done] Reset/restore paths: OP-11 clear (clear_all_asset_tables_drop) already
   drops+recreates ASSET_BY_ALBUM; the row-by-row reset_backend_state path
   clears it too; clear_asset_tables_and_caches drives the same drop path.
   Verified by test
   tasks::actor::album_index::tests::reset_rescan_reseeds_album_member_rows_and_aggregates:
   a DbLoss-style clear followed by a filesystem rescan re-seeds asset rows,
   member rows, and album aggregates exactly (I1 under ASS-3 — FS scan is
   authoritative and re-derives membership via ensure_dir_albums). Backstop
   (tasks/looper/backstop.rs) drives the same start_index_job("/", false, None)
   rescan, so it re-seeds identically. Partial-loss (asset rows without member
   rows) cannot occur on supported paths: every reset wipes ASSET_BY_ALBUM
   alongside the asset tables, and every membership mutation writes the member
   row in the same txn as the asset row.

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
