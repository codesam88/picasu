---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1b.

## Context

`start_watcher.rs` handles `Create` and `Modify` events; `Remove` events fall
through to `_ => {}`. A file deleted externally stays in the asset index as a
stale record with a broken canonical path. This breaks the filesystem-as-source-
of-truth promise.

## Tasks

- [ ] Handle `EventKind::Remove(_)` in the watcher: resolve the canonical path
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
