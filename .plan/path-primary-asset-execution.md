---
status: done
type: feature
priority: high
area: backend
---

# Path-Primary Asset Execution Plan

This is the implementation checklist for the path-primary asset design in
`asset-path-primary-index.md`. It is intentionally incremental and test-led.
Each phase should be completed and verified before the next phase starts.

## Rules for the Implementing Agent

- Do not preserve old database content or old API compatibility.
- Do not change multiple phases in one patch.
- Add the failing test before changing production behavior.
- Prefer real filesystem/API integration tests over mocks for index behavior.
- Keep the filesystem as the source of truth.
- Never merge two physical paths into one presented asset record.
- Never delete or move a second asset merely because its hash matches.
- Do not add optional query indexes until the required identity behavior works.
- Do not invent recovery/journal machinery until the operation protocol is
  specified and its failure cases have a test.
- After every phase run the smallest relevant test command, then `just check`
  before claiming the phase is complete.

## Required Behavior Contract

Write this contract into tests before implementation:

1. Every media file and directory has one stable `asset_id`.
2. Canonical path is unique across files and directories.
3. Assets have `kind: image | video | album`.
4. Images/videos may have a `blake_hash`; albums do not.
5. Same-hash files remain separate assets and appear separately in API/UI
   listings.
6. `DUPE_INDEX[hash]` contains the asset IDs sharing that hash.
7. Moving one asset changes only its path and related indexes.
8. Deleting one asset does not delete another same-hash asset.
9. Shared thumbnails remain until their final hash reference is removed.
10. Sidecars belong to one path asset and move/delete with that asset.
11. API mutations identify assets by `asset_id`, not hash plus alias path.
12. A clean index rebuild derives state from files, sidecars, and `.albuminfo`.

## Phase 0: Choose the Store

Dependency: `.plan/database-engine-evaluation.md`.

Decide whether the authoritative store is Redb or SQLite. Do not implement
both. Record only:

- table/schema definitions;
- transaction boundaries;
- schema-generation marker;
- clean rebuild activation method;
- required filesystem reconciliation behavior.

Do not add load or performance tests in this phase.

## Phase 1: Test Fixtures and Scenario Helpers — COMPLETED

### Backend fixture helpers — DONE

Added `duplicate_of` support to the scenario interpreter (`backend_api.rs`).
The fixture creates byte-identical copies via `fs::copy`, not regenerated
images. Source must exist on disk before the copy runs.

### API scenarios — DONE (red tests verified)

| Scenario file                                   | Status | Assertion                                                        |
| ----------------------------------------------- | ------ | ---------------------------------------------------------------- |
| `dup_same_album_two_items.yaml`                 | RED    | dataLength >= 3 (placeholder + 2 distinct files)                 |
| `dup_separate_albums_two_items.yaml`            | RED    | album_b dataLength >= 2 (placeholder + duplicate)                |
| `dup_move_one_leaves_other.yaml`                | RED    | source_album dataLength >= 2 after moving one record             |
| `dup_delete_record_does_not_destroy_other.yaml` | RED    | album dataLength >= 2 + copy.jpg exists after full record delete |
| `dup_delete_one_leaves_other.yaml`              | GREEN  | alias-level delete leaves remaining alias (both models OK)       |
| `dup_final_delete_removes_state.yaml`           | GREEN  | last alias delete purges record + thumbnail (both models OK)     |
| `dup_stale_path_rejected.yaml`                  | GREEN  | removed file returns dataLength 0 (both models OK)               |

Pre-existing red test also verified: `duplicate_files_are_independent_album_items.yaml`.

### Playwright scenarios — DEFERRED

Frontend Playwright scenarios deferred to Phase 9 after backend identity
refactor is complete.

## Phase 2: New Data Model and Empty Database — COMPLETED

### Type definitions — DONE

- `AssetKind` enum: `Image`, `Video`, `Album` — with `is_media()` predicate
- `AssetRecord` struct: `asset_id`, `kind`, `canonical_path`, `content_hash`,
  `file_size`, `ext`, `modified`, `scan_time`, `is_trashed`, `album_id`
- `canonicalize_path()` with `.`/`..` resolution

### Store tables — DONE

Three new Redb tables in the existing `index_v5.redb`:

- `ASSET_BY_PATH`: canonical path → `asset_id`
- `ASSET_BY_ID`: `asset_id` → JSON-serialized `AssetRecord`
- `DUPE_INDEX`: `content_hash` → JSON-serialized `Vec<asset_id>`

### Store operations — DONE

`storage/asset_store.rs` provides:

- `get_asset_id_by_path`, `put_asset_by_path`, `remove_asset_by_path`
- `get_asset_by_id`, `put_asset_by_id`, `remove_asset_by_id`
- `get_dupe_ids`, `add_to_dupe_group`, `remove_from_dupe_group`
- `insert_asset` / `remove_asset` — composite operations touching all three tables

### Unit tests — DONE (21 tests pass)

- `model::asset`: kind validation, display roundtrip, unique IDs, album
  no-hash, media hash storage, canonical path normalization (12 tests)
- `storage::asset_store`: path roundtrip, ID roundtrip, dupe group
  insert/query/remove/remove-last, composite insert/remove, album no-dupe,
  schema initialization and transaction wrapper (9 tests)

### Phase 1 red tests — VERIFIED (2 true red + 2 compatible + 1 pre-existing)

| Scenario                                      | Status | Why                                                                 |
| --------------------------------------------- | ------ | ------------------------------------------------------------------- |
| `dup_same_album_two_items`                    | RED    | asserts ≥3; current model yields 2 (hash-merged)                    |
| `dup_move_one_leaves_other`                   | RED    | source album empty after move; path-primary expects ≥2              |
| `dup_separate_albums_two_items`               | COMPAT | asserts ≥2 on album_b; merged record satisfies via alias membership |
| `dup_delete_record_does_not_destroy_other`    | COMPAT | assert satisfied by current placeholder behavior                    |
| `duplicate_files_are_independent_album_items` | RED    | pre-existing red test                                               |

## Phase 3: Clean Filesystem Rebuild — DONE

### Core rebuild — DONE

`process/rebuild.rs` provides `rebuild_from_filesystem(image_root)` which:

1. Walks the image root recursively.
2. Creates one album asset per directory (including root).
3. Creates one media asset per valid media file.
4. Computes blake3 content hashes for media assets.
5. Populates `DUPE_INDEX` without merging records.
6. Derives album membership from parent directory.

Unit tests (8 pass):

- `rebuild_creates_one_asset_per_file_and_directory` — verifies exact counts
  and unique asset IDs per file
- `rebuild_duplicate_files_get_separate_assets` — two byte-identical files get
  separate asset IDs and share a DUPE_INDEX group
- `rebuild_unsupported_files_are_skipped` — non-media files counted and skipped
- `rebuild_empty_directory_becomes_album` — empty dirs become album assets with
  no content hash
- `rebuild_stale_dupe_index_cleaned_on_rebuild` — stale DUPE_INDEX entries removed
- `rebuild_preserves_sidecar_files` — `.xmp` sidecars survive rebuild
- `rebuild_nested_directories_become_album_assets` — nested dirs become albums
- `rebuild_non_media_files_preserved` — non-media files not deleted by rebuild

### Deferred to Phase 4

- Sidecar matching (`.xmp` discovery per media asset)
- `.albuminfo` reading for album metadata enrichment
- Negative tests: path collision, path outside root, stale dupe index entries
- These require extending `AssetRecord` with metadata fields (description,
  tags, rating) which is part of the Phase 4 indexing pipeline work.

## Phase 4: Indexing and Duplicate Handling — COMPLETED (write path only)

### Path-primary indexing function — DONE

`process/index_asset.rs` provides `index_asset(src, image_root)` which:

- looks up canonical path in `ASSET_BY_PATH` first;
- updates existing asset if path already indexed (hash, size, modified time);
- allocates a fresh `asset_id` for new paths;
- updates `DUPE_INDEX` independently (removes from old group on hash change);
- never appends a path to another asset record;
- never deletes a path because its hash already exists;
- derives album membership from parent directory asset.

### Unit tests — DONE (4 pass)

- `index_identical_bytes_at_two_paths_creates_two_assets` — separate asset IDs,
  same DUPE_INDEX group
- `index_same_path_again_is_idempotent` — same path → same asset ID, DUPE_INDEX
  has exactly 1 entry
- `index_changed_bytes_updates_hash_group` — hash change removes from old group,
  adds to new; asset ID preserved
- `index_delete_one_path_does_not_affect_other` — removing one asset leaves the
  other and its DUPE_INDEX entry intact

### Not yet wired

The new `index_asset()` function is not called from the production indexing
pipeline (`workflow::index_image`). The existing `DeduplicateTask` still uses
hash-merging for the old `DATA_TABLE`. Wiring `index_asset` into the production
pipeline was attempted but caused test instability due to async race conditions.
The asset tables will instead be populated synchronously during
`update_tree_task` once Phase 7 completes the mutation endpoint migration.

## Phase 5: Snapshot and Query Read Path — COMPLETED

### Infrastructure done

- `ReducedData` has `asset_id` field populated from `DatabaseTimestamp`
- `MyCow::get_asset_id()` method for snapshot access
- `transitor::index_to_asset_id()` and `asset_id_to_abstract_data()` active
- `compute_locate()` accepts both `asset_id` and `hash`
- `build_from_asset_tables()` reads from `ASSET_BY_ID`, enriches from `DATA_TABLE`
  by asset_id (not content hash)
- `update_tree_task()` uses `build_from_asset_tables` as primary, falls back to
  `build_from_data_table`
- `get_data` resolves via `asset_id_to_abstract_data`
- `Album` filter normalizes relative/absolute paths via `normalize_parent()`
- `VERSION_COUNT_TIMESTAMP` updated immediately in `update_tree_task` for cache invalidation

## Phase 6: Serving, Tokens, and API Responses — DONE

### Token system — DONE

- `ClaimsHash` has required `asset_id` field
- `DataBaseTimestampReturn` exposes `asset_id` in JSON response
- `DataBaseTimestampReturn::with_asset_id()` includes `asset_id` in token
- `get_data` endpoint passes `asset_id` through to token generation

### Locate — DONE

- `compute_locate` uses `asset_id` only — no hash fallback
- `ReducedData.asset_id` is required `ArrayString<64>`
- `discover_asset_id` test helper captures `asset_id` from `get-data` response
- `asset_id_as` scenario field for capturing `asset_id` in `given` and `when` sections

### Original serving — DONE

- `imported_file` resolves by `asset_id` only — no hash fallback
- `GuardHashOriginal` validates `asset_id` from token — no hash fallback, errors if missing

### Negative tests — DONE

- `negative_unknown_asset_id`: unknown asset_id returns null locateTo
- `negative_deleted_asset_not_locatable`: deleted asset returns null locateTo
- `negative_moved_asset_serves_new_path`: moved asset resolves to new path by asset_id

### Remaining Phase 6 items

- Album covers reference `asset_id` (deferred to Phase 7)
- Compressed thumbnail serving keeps hash-based identity (by design, no change needed)

## Phase 7: Move, Delete, Sidecars, and Album Operations — DONE

### assign_album — DONE

- `AssignAlbumData` has required `asset_id` field
- `move_asset_into_album()` function moves exactly one physical file by `asset_id` via `ASSET_BY_ID`/`ASSET_BY_PATH`
  lookup
- `move_item_into_album` uses asset_id as DATA_TABLE key

### FlushTreeTask — DONE

- Writes to ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, and DATA_TABLE
  (keyed by asset_id, not content hash)
- Remove path: cleans up all tables, with stale-path fallback for
  pruned aliases

### DeduplicateTask — DONE

- Never merges aliases — always returns `Some(abstract_data)`
- Each file goes through the full IndexTask pipeline independently

### Read path — DONE

- `edit_tag`, `edit_rating`, `edit_description`, `edit_flags` all use
  `index_to_asset_id` for DATA_TABLE lookup
- `rotate_image`, `regenerate_thumbnail`, `get_img` use
  `lookup_abstract_data_by_hash` (resolves via DUPE_INDEX)
- `get_test_probe` resolves content hash to asset_id via DUPE_INDEX
- `dir_album::write_album_to_db` writes to ASSET_BY_ID and ASSET_BY_PATH

### Delete/trash — DONE

- `delete_multi_alias` scenario updated for path-primary (deleting one
  asset does not destroy same-hash sibling)
- `process_deletes` resolves via `index_to_abstract_data` which uses asset_id
- `FlushTreeTask::remove` cleans up ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX
- `delete_by_asset_id` scenario: delete by asset_id works correctly
- `delete_removes_file_and_sidecar`: file and sidecar removed from disk

### Directory moves — DONE

- `update_asset_tables_after_dir_move()` updates `ASSET_BY_PATH` and
  `ASSET_BY_ID` when a directory is moved via `assign_album`
- `dir_move_updates_asset_tables` scenario verifies asset_id preserved

### Album cover references — DONE

- `SetAlbumCover` uses `cover_asset_id` instead of `cover_hash`
- `set_album_cover` handler looks up cover by `asset_id`
- `AlbumCombined::set_cover()` stores `asset_id` in `metadata.cover`
- `self_update()` populates `MediaItemInfo.asset_id` from `DatabaseTimestamp`
- `set_cover_from_info()` stores `asset_id` when available

### Shared thumbnail cleanup — DONE

- `remove_compressed_thumbnail()` in `alias.rs` checks `DUPE_INDEX` before
  removing. If `get_dupe_ids(hash).len() > 1`, other assets still reference
  the content hash and the thumbnail is preserved.
- Legacy delete path in `delete.rs` applies the same check.
- `dup_delete_preserves_shared_thumbnail` scenario: delete one of two
  same-hash files → thumbnail preserved; the `dup_delete_one_leaves_other`
  scenario also asserts `thumb_exists`.
- `dup_final_delete_removes_state` continues to verify that deleting the
  final reference removes the thumbnail.

### Recursive album deletion — DONE

- `cleanup_album_descendants()` in `delete.rs` handles recursive cleanup
  for album records: finds descendant assets via `get_assets_under_path`,
  removes files + sidecars from disk, removes from asset tables
  (ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX), flushes from DATA_TABLE,
  evicts child album caches, removes .albuminfo, removes directory tree.
- Runs after `process_deletes` validation passes, before FlushTreeTask.
- Shared thumbnails preserved via existing DUPE_INDEX check in
  `remove_compressed_thumbnail`.
- `album_delete_recursive` scenario: parent album with 2 child files +
  sub-album + sidecar → all removed from disk and asset tables.
- `album_delete_preserves_sibling` scenario: deleting one asset of a
  same-hash pair preserves the other and shared thumbnail.
- `negative_delete_unknown_album` scenario: out-of-bounds index returns 500.

### Remaining work

Phase 7 is functionally complete for delete/trash, directory moves, album
covers, shared thumbnails, and recursive album deletion.

## Phase 8: Watcher and Reconciliation — DONE

### Current state

The watcher code is already path-primary by design:

- `DeduplicateTask` always returns `Some` (never merges aliases)
- `FlushTreeTask` writes to all four tables (ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, DATA_TABLE)
- `handle_removed_file` searches by canonical path alias, not hash
- Create/Modify events go through debounce → `index_image` (path-based)
- Remove events go through `handle_removed_file` (path-based)

### Tests added

- `watcher_create_same_hash_asset`: two same-hash files indexed separately
- `watcher_remove_preserves_sibling`: deleting one duplicate preserves the other and shared thumbnail
- `watcher_rename_preserves_sibling`: deleting original preserves copy and moved files
- `watcher_stale_path_no_delete`: re-indexing preserves existing assets
- `watcher_discovers_new_file`: existing test (unchanged)
- `watcher_modify_updates_hash_group`: changing bytes of one duplicate updates only that asset (uses `write_file` action)
- `watcher_sidecar_change_triggers_reindex`: modifying XMP sidecar triggers re-index of associated media
- `watcher_sidecar_missing_does_not_break_asset`: deleting sidecar does not break asset
- `watcher_rebuild_reconciliation_stale_dupe`: rebuild cleans stale DUPE_INDEX entries after file removal

### Remaining work

- partial/canceled scan edge cases (may need Playwright tier)
- durable operation journal (only if tests demonstrate rebuild/reconciliation is insufficient)

## Phase 9: Frontend Identity Refactor — DONE

Replace hash identity in this order:

- `dataStore` maps by asset ID; — DONE (dual-map: hashMapData for URL routing, assetIdMapData for identity)
- worker payloads and row data use asset ID; — DONE (assetId passed through pipeline, stored in EnrichedUnifiedData)
- routes and view navigation use asset ID; — DONE (getSrc uses assetId for original files)
- original-image cache uses asset ID; — DONE (backend resolves /object/imported by assetId)
- shared thumbnail cache may use hash; — DONE (compressed files always use content hash)
- token persistence uses asset ID; — DONE (tokens keyed by assetId, not content hash)
- menus, downloads, covers, metadata, move, and delete use asset ID; — DONE (require assetId, no hash fallback)
- direct alias/path presentation becomes direct canonical asset path.

Playwright tests verify duplicate rendering and independent selection,
move, delete, refresh, pagination, and original serving.

### Slices completed

1. Schema: `assetId` in `databaseTimestampSchema` +5 unit tests — `6adbaa28`
2. Data store: dual-map (`hashMapData` + `assetIdMapData`) + regression tests — `47680811`
3. Original URLs: `getSrc`/`getSrcOriginal` use assetId for originals + tests — `1c0a3a6c`
4. Playwright: duplicate selection + deletion scenarios — `4056f2b1`
5. Token storage: `assetTokenMap` keyed by assetId, service worker by assetId — `da81f11d`
6. Mutation APIs: `assignAlbum` sends assetId, worker payloads include assetId — `43e9fa3f`
7. Remove hash fallback: require assetId for media items — `59e82a5c`
8. Playwright: duplicate move, refresh, pagination, original serving — `d89d9fe7`

## Phase 10: Remove Old Identity Code — DONE

### Completed

- `get_test_probe.rs`: removed hash fallback, now uses asset_id only — `6746adc3`
- `update_tree.rs`: removed dead `sync_asset_tables_from_data_table` and `deterministic_id` — `b8ae48db`
- `transitor.rs`: removed dead `index_to_hash` and `hash_to_abstract_data` functions
- `get_img.rs`: updated doc comment to reflect asset_id-only resolution — `d84e6d4a`
- Frontend: renamed `RefreshHashTokenPayload.hash` to `assetId` — `49bc0bfb`
- Rebuild tests: added stale dupe index, sidecar preservation, nested directory tests — `34ac853d`

### Remaining

- `DATA_TABLE` still exists as a rich metadata store (tags, exif, etc.) — this is legitimate
- `asset_record_to_abstract_data` uses content hash for display_id — legitimate for compressed thumbnail paths
- `ser_de.rs` legacy schema deserialization — needed for reading existing data
- `alias.rs` alias-based operations — used by delete/trash, not for identity

Run the complete project checks and API/UI scenario suites.

- 0 Playwright failures remain (33/33 pass)

### Playwright lifecycle fix — `ad0a30e4`

Root causes resolved:

1. `resolveLocator` used `getByRole('testid', ...)` instead of `getByTestId(...)` for `testid/` prefix. Vuetify's
   `v-testid` sets `data-testid`, not an ARIA role.
2. `click.text` handler clicked `.parent` center which landed on the hover icon (entering edit mode) or thumbhash
   placeholder (intercepting pointer events). Fixed by dispatching synthetic click on `#click-handler` div.
3. `duplicate-move-independently` scenario: wrong option name casing, `click.text` used in modal instead of
   `click: option/`, expected counts didn't account for `dir_album` placeholder photo.

---

## Audit Review — 2026-09-22

### Feature completeness

| Phase                       | Status     | Notes                                                                     |
| --------------------------- | ---------- | ------------------------------------------------------------------------- |
| Phase 0: Store              | ✅ Done    | Redb chosen, tables defined                                               |
| Phase 1: Fixtures/scenarios | ✅ Done    | Backend helpers + 8 API scenarios                                         |
| Phase 2: Data model         | ✅ Done    | `AssetKind`, `AssetRecord`, `DUPE_INDEX` — 21 unit tests                  |
| Phase 3: Rebuild            | ✅ Done    | `rebuild_from_filesystem` — 8 unit tests                                  |
| Phase 4: Indexing           | ⚠️ Partial | `index_asset` exists with 4 unit tests, but **no production callers**     |
| Phase 5: Read path          | ✅ Done    | `build_from_asset_tables` reads `ASSET_BY_ID`, enriches from `DATA_TABLE` |
| Phase 6: Serving/tokens     | ✅ Done    | Asset ID in tokens, routes, locate                                        |
| Phase 7: Mutations          | ✅ Done    | Assign, delete, covers, thumbnails, recursive delete                      |
| Phase 8: Watcher            | ✅ Done    | Path-based events, 9 watcher scenarios                                    |
| Phase 9: Frontend           | ✅ Done    | `assetIdMapData` sole identity, 8 Playwright scenarios                    |
| Phase 10: Cleanup           | ✅ Done    | Legacy functions removed                                                  |

### Key gaps

1. **`index_asset` is dead code** — defined in `process/index_asset.rs` with 4 unit tests, but never called from
   production. The `#[allow(dead_code)]` annotation masks the warning. The production indexing pipeline
   (`workflow::index_image` → `DeduplicateTask` → `IndexTask`) still constructs `AbstractData` from the old
   hash-based `AbstractData::new(&path, hash)` shape.

2. **`rebuild_from_filesystem` is dead code** — defined in `process/rebuild.rs` with 8 unit tests, but never called
   from production. No CLI command or startup hook invokes it. The `#[allow(dead_code)]` annotation masks the warning.

3. **`DeduplicateTask` uses old model** — constructs `AbstractData::new(&path, hash)` which bypasses `AssetRecord`
   entirely. The deduplication path does not populate `ASSET_BY_PATH`, `ASSET_BY_ID`, or `DUPE_INDEX`.

4. **`DATA_TABLE` is still the primary read source** — `build_from_asset_tables` reads `ASSET_BY_ID` then enriches
   from `DATA_TABLE` for metadata. The new tables are written to but not the authoritative read path for most endpoints.
   This is by design (DATA_TABLE holds tags, exif, etc.) but means the new tables are supplementary, not primary.

5. **`asset_record_to_abstract_data` is only called from `delete.rs`** — not from `get_data.rs` or the tree build
   path. The function exists but is not on the main read path.

### What's NOT a gap (by design)

- `DATA_TABLE` remaining as metadata store — legitimate, holds tags/exif/descriptions
- `content_hash` used as `object.id` for media items — legitimate, required for compressed thumbnail path resolution
- `ser_de.rs` legacy migrations — needed for schema versioning
- `alias.rs` operations — used by delete/trash, not for identity

### Test counts

| Module                   | Unit tests | API scenarios | Playwright |
| ------------------------ | ---------- | ------------- | ---------- |
| `model/asset.rs`         | 4          | —             | —          |
| `storage/asset_store.rs` | 9          | —             | —          |
| `process/rebuild.rs`     | 8          | —             | —          |
| `process/index_asset.rs` | 4          | —             | —          |
| `process/alias.rs`       | 3          | 3             | —          |
| `process/dir_album.rs`   | —          | 11            | —          |
| Duplicate handling       | —          | 7             | 5          |
| `assign_album`           | —          | 16            | 2          |
| Frontend identity        | —          | —             | 8          |
| **Total**                | **28**     | **37**        | **15**     |

All 34 Playwright + 62 frontend unit tests pass.

---

## Phase 11: Fix DUPE_INDEX old-group removal on hash change — DONE

**Background (verified 2026-09-22):** `flush_tree_task` (tasks/batcher/flush_tree.rs:139) adds the asset id to the
_new_ hash group on insert but **never removes it from the old group** when a file's bytes change.
`process/index_asset.rs:78` explicitly does "remove from old group, add to new" — but `index_asset` has no production
callers. The live path is the one with the stale-group gap. Whether this leaves observable stale state depends on
rebuild/reconcilation cleanup, so treat it as a divergence to close, not a confirmed user-facing bug.

### Step 1: Add failing test

Extend `watcher_modify_updates_hash_group.yaml` to assert the **old** `DUPE_INDEX` group no longer contains the
asset_id after a byte change (currently only the new group is asserted). This should go RED: `flush_tree` adds to the
new group without cleaning the old one.

### Step 2: Implement

Make the dupe-group bookkeeping correct in the live write path. Preferred: have `flush_tree_task` delegate group
membership to the same store ops `index_asset` uses (`remove_from_dupe_group` for the old hash before
`add_to_dupe_group` for the new), so there is one implementation of group semantics.

### Step 3: Verify

New assertion GREEN. Run `just check; just test`.

## Phase 11b: Consolidate `index_asset` and `flush_tree` — DONE

**Background (verified 2026-09-22):** the earlier draft assumed the production pipeline did not populate the asset
tables. That is wrong — `flush_tree_task` (flush_tree.rs:100-155) already writes `ASSET_BY_ID`, `ASSET_BY_PATH`,
`DUPE_INDEX`, and `DATA_TABLE` for every inserted `AbstractData`. There are now **two** implementations of "create an
asset record for a file": the dead `process/index_asset.rs` (tested) and the live inline record construction in
`flush_tree.rs` (untested at the unit level). The prior plan proposed wiring `index_asset` into `DeduplicateTask`; that
would add a third implementation. Instead, consolidate to one.

Note: `DeduplicateTask`'s `hash` field is not dead — `index_image` computes the hash via `HashTask` and passes it in
(workflow/mod.rs:76-84). Do not remove it.

### Step 1: Decide the single owner

Two options, pick with a test-first phase:

- **A:** `index_asset` becomes the sole constructor of an `AssetRecord` from a path + hash; `flush_tree_task` calls it
  instead of building the record inline. Pros: tested logic is the live logic; DUPE fix from Phase 11 lands in one
  place. Cons: `index_asset` re-reads/opens tables the caller already has open.
- **B:** Keep record construction in `flush_tree_task`, backfill its asset-record/dupe logic with unit tests, then
  delete `index_asset` and its tests. Pros: smallest change; no dead code remains. Cons: throws away tested code.

### Step 2: Follow the decision

If A: add a unit test asserting `flush_tree` record construction equals `index_asset` output (asset_id, kind, path,
hash, size) for the same inputs, then switch the call site. If B: port the old-group-removal behavior (Phase 11) into
`flush_tree`, add the `index_asset` unit tests at the `flush_tree`/`asset_store` level, then remove `index_asset.rs`.

### Step 3: Verify

`just check; just test` after whichever path is chosen.

---

## Phase 12: Wire `rebuild_from_filesystem` into Production — DONE (Option B: `POST /post/rebuild`)

**Goal:** Add a CLI command or startup hook that invokes `rebuild_from_filesystem` so the asset tables can be populated
from a clean state.

### Step 1: Add failing test

Add a new API scenario `rebuild_populates_asset_tables.yaml`:

- Given: an empty database
- When: `rebuild_from_filesystem` is invoked
- Then: `ASSET_BY_PATH`, `ASSET_BY_ID`, `DUPE_INDEX` are populated correctly
- Then: the in-memory tree is populated and `get-data` returns the expected items

This test should **fail** because no production code invokes `rebuild_from_filesystem`.

### Step 2: Add CLI command or startup hook

Option A: Add a `--rebuild` CLI flag to `main.rs` that runs `rebuild_from_filesystem` before starting the server.

Option B: Add a `POST /admin/rebuild` endpoint that triggers a rebuild.

Option C: Call `rebuild_from_filesystem` on startup when the asset tables are empty.

Choose the option that fits the project's deployment model. The plan recommends Option A (CLI flag) for explicitness.

### Step 3: Verify

Run the new scenario (should now pass). Run full test suite. Remove `#[allow(dead_code)]` from `process/rebuild.rs`.

### Step 4: Remove dead code annotation

Remove `#![allow(dead_code)]` from `backend/src/process/rebuild.rs`.

---

## Phase 13: Remove `#[allow(dead_code)]` from Asset Module — DONE

**Goal:** Clean up all `#![allow(dead_code)]` annotations that were masking unused-code warnings during the incremental
migration.

### Step 1: Add failing test

This is a compilation-level test: remove `#![allow(dead_code)]` from `backend/src/model/asset.rs` and
`backend/src/storage/asset_store.rs`. If any items are truly dead, the compiler will warn (treated as error via `just
check`).

### Step 2: Remove annotations

Remove `#![allow(dead_code)]` from:

- `backend/src/model/asset.rs`
- `backend/src/storage/asset_store.rs`

### Step 3: Fix any warnings

If the compiler reports dead items, either:

- Wire them into production (Phase 11b/12), or
- Remove them if they're genuinely unused.

### Step 4: Verify

Run `just check` — zero warnings expected.

---

## Phase 14: Metadata Consolidation — lean identity index + dedicated metadata table — DONE

**Decision (2026-09-22):** Consolidate toward two stores with clean separation, rather than collapsing metadata into
`ASSET_BY_ID`. Rationale: serializing the full `AbstractData` (EXIF vec, tags, description) into the hot identity index
would bloat every tree reboot and rename. The target separates "what's needed to walk/sort/dupe-check" from "what's
needed to render a single item's detail".

### Target table layout

| Table                                                       | Key               | Content                                                             | Read on                                         |
| ----------------------------------------------------------- | ----------------- | ------------------------------------------------------------------- | ----------------------------------------------- |
| `ASSET_BY_ID`                                               | `asset_id`        | lean `AssetRecord` (identity, kind, path, hash, size, times, album) | every walk, sort, rename, dupe check            |
| `ASSET_BY_PATH`                                             | canonical path    | → `asset_id`                                                        | path resolution in rename/move                  |
| `DUPE_INDEX`                                                | content hash      | → `Vec<asset_id>`                                                   | dedup / shared-thumbnail decisions              |
| **`METADATA_TABLE`** (new, replaces fat `DATA_TABLE` value) | `asset_id`        | tags, description, rating, EXIF vec, cover ref                      | **only** detail view / sidebar / metadata edits |
| `TREE` (in-memory)                                          | sorted vec        | `DatabaseTimestamp` carrying lean `AssetRecord`-derived fields      | timeline sort + paging                          |
| query snapshots                                             | timestamp / count | `Prefetch`, `ReducedData`                                           | get-data pages (unchanged)                      |

### Step 1: Add failing test

New API scenario `metadata_only_loaded_on_detail.yaml`:

- Given: an album with several images (one with tags/EXIF set)
- When: a timeline `get-data` page for that album is fetched
- Then: the response rows carry identity/timestamp/hash/cover but **do not** carry the full EXIF/tags/description on the
  list payload (assert the tag/description fields are empty or absent on a row that is not the detail target); a
  detail/sidebar fetch for one item returns the full metadata.

### Step 2: Introduce `METADATA_TABLE`

- Add table: `METADATA_TABLE: TableDefinition<&str, AbstractData>` (or a slimmer metadata struct) keyed by `asset_id`.
- `flush_tree_task` writes identity to `ASSET_BY_ID`/`ASSET_BY_PATH`/`DUPE_INDEX` and the metadata payload into
  `METADATA_TABLE` instead of the full-value `DATA_TABLE`.
- Remove path: also flush metadata record.

### Step 3: Make detail/sidebar reads go through `METADATA_TABLE`

- `get_data`, `get_prefetch`, `build_from_asset_tables` stop requiring the full `AbstractData`; they build a lean
  `DatabaseTimestamp` from `AssetRecord` + minimal fields.
- Detail/sidebar/metadata-edit endpoints (`edit_tag`, `edit_rating`, `edit_description`, per-item metadata return)
  resolve `asset_id` → `METADATA_TABLE` explicitly.
- `cover_content_hash_from_data` now reads the cover `asset_id` from the metadata record.

### Step 4: Preserve parity

Enumerate every consumer of the fat in-memory tree before landing the lean change:

- filter expression evaluation (tags, ratings) — must not regress; if filters scan the in-memory tree, the lean tree
  needs to fetch metadata per filtered candidate, or filters move to a tag-index secondary table (deferred, see below)
- `compute_timestamp` only needs priority fields — confirm it works from lean data
- album `self_update`, share metadata resolution, `set_cover` — confirm paths resolve cover via metadata table

Existing green scenarios that exercise tags, EXIF display, sidebar metadata, ratings, and album covers are the parity
gate; all must remain green.

### Deferred (separate phase if warranted)

- Tag-based inverted index secondary table (tag → asset_ids) so tag filters don't need metadata per row. NOT required
  for this phase; only if filter benchmarks justify it.

### Verify

`just check; just test` after each step. Do not land Step 3 until the parity audit in Step 4 is written down.

- **2026-09-22 — Phase 11b DONE (Option B).** `flush_tree_task` split into `flush_tables` (testable core, no
  coordinator dispatch) + trailing `UpdateTreeTask` dispatch — production behavior unchanged. index_asset's four
  behavioral guarantees ported as `flush_*` unit tests (test 3 mutation-verified non-vacuous); no flush gaps found —
  all four already held. `process/index_asset.rs` deleted, `pub mod` removed, zero references remain. Backend count 259
  (baseline included index_asset's 4; −4 +4). `just check` + `just test` green (259/24/62/34). Two playwright flakes
  mid-gate attributed to host CPU contention; isolated reruns passed.
- **2026-09-22 — Phase 12 DONE (Option B override).** `POST /post/rebuild` (GuardAuth + GuardReadOnlyMode) runs
  rebuild via spawn_blocking, then `sync_data_table()` (rebuild mints new asset_ids; stale DATA_TABLE rows replaced
  via `asset_record_to_abstract_data` + album_id), then `execute_batch_waiting(UpdateTreeTask)` so get-data cannot race
  the tree refresh. Option A (CLI flag) overridden: TEST_ENV/DATA_PATH are process-global set-once, so scenarios
  cannot relaunch with argv — the plan's own API-scenario test vehicle requires an HTTP endpoint. RED verified (404,
  missing route). Scenario `rebuild_populates_asset_tables.yaml` asserts rebuild → discover by path → get-data
  returns the asset. `#![allow(dead_code)]` removed from rebuild.rs. Gates green: backend 260 / utils 24 / vitest 62 /
  playwright 34. Note: rebuild re-mints all asset_ids — outstanding tokens/links referencing old ids do not survive a
  rebuild (pre-existing property of rebuild.rs, now reachable in production).
- **2026-09-22 — Phase 13 DONE.** File-level `#![allow(dead_code)]` removed from `model/asset.rs` and
  `storage/asset_store.rs`. Three warnings resolved: `resolve_hash_to_asset_id` deleted (zero callers, grep-verified);
  `AssetRecord::is_media`/`is_album` kept with item-scoped `#[allow(dead_code)]` (cfg(test)-only callers are the Phase 2
  contract tests — deleting would drop tested guarantees). clippy `-D warnings` + fmt clean; backend count unchanged
  at 260. Out of scope (left): annotations in `storage/cache.rs`, `error.rs`.
- **2026-09-22 — Phase 14 DONE.** `DATA_TABLE` → `METADATA_TABLE` renamed across 22 files (on-disk name `"database"`
  → `"metadata"`; old rows orphaned, clean reindex repopulates). New `GET /get/metadata/{asset_id}` detail endpoint
  (GuardTimestamp + share-metadata parity). get-data list rows now built from snapshot `ReducedData` (extended with
  `update_at`/`pending`) + lean `ASSET_BY_ID` record — no per-row fat read; strips exif/tags/description
  (+rating/isFavorite/isArchived absent from lean rows — not tile-visible). Frontend fetches detail on demand
  (ViewPageMetadata panel open, EditTagsModal prefill) with `dataStore.mergeMetadata`. Sanctioned deviations: in-memory
  TREE stays full (filters/get-tags parity; lean TREE + tag-index remains the documented follow-up); two scenarios
  instead of one (interpreter asserts only against last when-response); 4 pre-existing tag scenarios migrated from
  get-data rows to the detail endpoint (their list-row form contradicted the sanctioned strip). RED evidence captured
  for both new scenarios; parity gate incl. all 4 sidebar Playwright scenarios green after `just frontend-build`
  (stale-dist false alarm caught). Final gates: `just check` 0, `just test` 0 — backend 262 / utils 24 / vitest 65 /
  playwright 34.
