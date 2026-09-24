---
status: done
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1b.

## Context

`start_watcher.rs` now handles `Create`, `Modify`, and `Remove` events. An
external deletion resolves the canonical path through `ASSET_BY_PATH` and
removes that asset (plus `DUPE_INDEX` membership and derived state), keeping
the filesystem as the source of truth.

## Tasks

- [x] Handle `EventKind::Remove(_)` in the watcher: resolve the canonical path
      through `ASSET_BY_PATH`, remove that asset from the asset tables and
      `DUPE_INDEX`, and remove derived state when no asset still references it.
- [x] On manual album indexing (`POST /post/index/album`), the sweep after scanning new files should also check existing
      asset records under the target path for missing canonical files.

## Progress (2026-09-07)

- Task 2 (sweep on manual album index) shipped in PR \#17 via the stale-path
  sweep in `album_index.rs`; E2E scenario `album_index_removes_stale_paths.yaml`
  passes.
- Watcher `Remove` handling (task 1) remains open.
- Locking/ordering concerns in the sweep are tracked separately in `album-index-sweep-concurrency.md`.

## Progress (2026-09-24)

- Task 1 completed by the path-primary work (Phase 8): `EventKind::Remove(_)`
  routes to `handle_removed_file`, which resolves the canonical path and prunes
  the asset through `prune_asset_path` (asset tables, `DUPE_INDEX`, derived
  state); covered by the watcher reconciliation scenarios.
- Closed as done.
