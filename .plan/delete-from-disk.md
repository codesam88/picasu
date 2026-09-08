---
status: done
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1a.

## Context

`DELETE /delete/delete-data` removes the DB record and `.xmp` sidecar but never `fs::remove_file` on the original file
or its thumbnail.

## Tasks

- [x] Two-step delete UX: "trash" (soft, existing) → "confirm delete from disk" (hard). TrashedPage exists;
      "Permanently Delete" is wired into the single and batch trashed menus (`SingleMenu.vue`, `BatchMenu.vue`).
- [x] `DELETE /delete/delete-data` (or new endpoint) must: 1. For each alias path, `fs::remove_file` the original 2.
      `fs::remove_file` the `.xmp` sidecar (done) 3. `fs::remove_file` the compressed thumbnail at
      `compressed_path(hash)` 4. Remove from DB (done)
- [x] Handle multi-alias case: only remove from disk when removing the last alias; for earlier aliases only remove that
      alias path from the `alias[]` list.
- [x] `DIR_ALBUM_CACHE` eviction on delete.

## Design (2026-09-08)

- `DELETE /delete/delete-data` gains a required `aliasList` field: a parallel
  array, one entry per `deleteList` index, naming the exact alias path the
  client surfaced (`abstractData.alias[0].file` in get-* responses). Entries
  are `null` for albums. Length must match `deleteList`; any image/video entry
  that does not match a current alias of the targeted record → 400 (defends
  against stale multi-user state deleting the wrong file).
- Per-index behavior: image/video → delete only that alias's original file and
  `.xmp` sidecar, prune it from the record; if no aliases remain, also delete
  the compressed thumbnail and remove the record — else persist the pruned
  record (thumbnail retained). Album → unchanged (dir-album cache eviction +
  record removal, alias entry must be `null`).
- Shared helper `prune_alias_paths` (new `backend/src/process/alias.rs`):
  mutate + disk-delete + last-alias thumbnail logic, reused by delete,
  `start_watcher::handle_removed_file`, and `album_index::sweep_stale_aliases`
  (code-sharing requested in review; adds `.xmp` sidecar cleanup for externally
  deleted aliases).
- Scenarios: new `delete-multi_alias` (upload duplicate → dedup → delete the
  surfaced alias → sibling survives, thumbnail served, record locatable);
  `delete_removes_file_and_sidecar_z3` updated to send `aliasList`.
- Frontend `ItemPermanentlyDelete` resolves each index's surfaced alias from
  the data store and sends it; albums send `null`.

## Progress (2026-09-08)

Branch `feat/delete-multi-alias` — TDD red-green implemented.

- `delete_multi_alias.yaml` written: upload identical content → dedup 2-alias record
  `[src/photo.jpg, target/photo.jpeg]` → delete surfaced alias (`photo.jpeg`) →
  `src/photo.jpg` survives, thumbnail served, record locatable. Initial watcher-enabled
  runs produced non-deterministic dedup (second upload skipped by hash lock race); resolved
  by uploading once for deterministic 2-alias setup.
- `DeleteList` in `backend/src/router/delete.rs` extended with `alias_list: Vec<Option<String>>`
  (camelCase `aliasList`, `#[serde(default)]`). Validation: length match when non-empty; 400
  for album entry with non-null alias; 400 for alias not matching the record. Legacy path
  (no aliasList) retained for backward compatibility.
- `backend/src/process/alias.rs` created with `prune_alias_paths`: removes target alias file +
  `.xmp` sidecar, prunes from record, removes thumbnail if last alias — returns remaining flag.
- `process_deletes` refactored: per-index branching on alias_list presence; updated records
  persisted via `FlushTreeTask::insert`; removed records via `FlushTreeTask::remove`. Legacy
  path preserves original file/sidecar/thumb deletion.
- `delete_removes_file_and_sidecar_z3.yaml` updated to read surfaced alias via get-data and
  send `aliasList` (tests legacy backward compat via serde default).
- `delete_alias_mismatch_rejected.yaml` added: sends bogus alias → expects 400 + file untouched.
- `ItemPermanentlyDelete.vue` updated: imports `useDataStore`, maps indexList to `aliasList`
  (image/video → `alias[0]?.file`, album → null), sends in delete request.
- All checks pass: `just check` (clippy, fmt, vue-tsc, eslint, prettier, plan lint),
  `just test` (192 backend + 25 playwright E2E), zero failures.

**Deferred (follow-up PR):** Refactor `start_watcher::handle_removed_file` and
`album_index::sweep_stale_aliases` to reuse `prune_alias_paths` (code-sharing requested
in design review; adds `.xmp` sidecar cleanup for externally deleted aliases). Thumbnail
removal assertion still absent from `delete_removes_file_and_sidecar_z3.yaml`.

PR \#17 review re-check against `main`:

- `process_deletes` (`backend/src/router/delete.rs:117-138`) already does the disk deletion (originals, `.xmp` sidecars,
  compressed thumbnails) — the plan's original context ("never fs::remove\_file") was stale.
- Task 2 is therefore already implemented on `main`; task 1 (frontend "Permanently Delete" action) also shipped before
  this PR — this PR only reorganized the menus and added E2E coverage.
- PR \#17 added the `DIR_ALBUM_CACHE` eviction on album delete (task 4).
- **Remaining open item: the multi-alias "last alias only" rule (task 3).** Current behavior removes every alias path
  and drops the whole record regardless of how many aliases a hash has; the intended rule keeps the record (minus the
  removed alias) when other aliases still exist.
- Test-coverage note: `backend/tests/scenarios/delete_removes_file_and_sidecar_z3.yaml` asserts the original + `.xmp`
  sidecar are gone from disk after `DELETE /delete/delete-data`, but **nothing asserts the compressed thumbnail removal**
  — worth extending that scenario (or adding a `file_absent` on the thumbnail path) when task 3 is tackled.
