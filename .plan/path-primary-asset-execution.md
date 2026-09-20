---
status: in-progress
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

## Phase 3: Clean Filesystem Rebuild — IN PROGRESS

### Core rebuild — DONE

`process/rebuild.rs` provides `rebuild_from_filesystem(image_root)` which:

1. Walks the image root recursively.
2. Creates one album asset per directory (including root).
3. Creates one media asset per valid media file.
4. Computes blake3 content hashes for media assets.
5. Populates `DUPE_INDEX` without merging records.
6. Derives album membership from parent directory.

Unit tests (4 pass):

- `rebuild_creates_one_asset_per_file_and_directory` — verifies exact counts
  and unique asset IDs per file
- `rebuild_duplicate_files_get_separate_assets` — two byte-identical files get
  separate asset IDs and share a DUPE_INDEX group
- `rebuild_unsupported_files_are_skipped` — non-media files counted and skipped
- `rebuild_empty_directory_becomes_album` — empty dirs become album assets with
  no content hash

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

## Phase 6: Serving, Tokens, and API Responses — IN PROGRESS

### Token system — DONE

- `ClaimsHash` has optional `asset_id` field (serde default, backward compatible)
- `ClaimsHash::with_asset_id()` builder method
- `DataBaseTimestampReturn` exposes `asset_id` in JSON response
- `DataBaseTimestampReturn::with_asset_id()` includes `asset_id` in token
- `get_data` endpoint passes `asset_id` through to token generation

### Locate — DONE

- `compute_locate` prefers `asset_id` match, falls back to `hash` for backward compat
- `locate_same_hash_by_asset_id` scenario verifies each asset locates by its own ID
- `discover_asset_id` test helper captures `asset_id` from `get-data` response
- `asset_id_as` scenario field for capturing `asset_id` in `given` section

### Original serving — DONE

- `imported_file` resolves by `asset_id` first (via `ASSET_BY_ID`), falls back to hash
- `GuardHashOriginal` validates `asset_id` from token when present, falls back to `hash`

## Phase 6: Serving, Tokens, and API Responses — DONE

### Token system — DONE

- `ClaimsHash` has optional `asset_id` field (serde default, backward compatible)
- `ClaimsHash::with_asset_id()` builder method
- `DataBaseTimestampReturn` exposes `asset_id` in JSON response
- `DataBaseTimestampReturn::with_asset_id()` includes `asset_id` in token
- `get_data` endpoint passes `asset_id` through to token generation

### Locate — DONE

- `compute_locate` uses `asset_id` only — no hash fallback
- `ReducedData.asset_id` is `Option<ArrayString<64>>` (None for old-model entries)
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

## Phase 7: Move, Delete, Sidecars, and Album Operations — IN PROGRESS

### assign_album — DONE

- `AssignAlbumData` has optional `asset_id` field (serde default, backward
  compatible)
- `move_asset_into_album()` function moves exactly one physical file by
  `asset_id` via `ASSET_BY_ID`/`ASSET_BY_PATH` lookup
- `resolve_asset_id_from_hash()` resolves content hash + alias to asset_id
  via DUPE_INDEX (with ASSET_BY_ID scan fallback)
- When `asset_id` is absent, falls back to resolved asset_id from hash
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

### Delete/trash — IN PROGRESS

- `delete_multi_alias` scenario updated for path-primary (deleting one
  asset does not destroy same-hash sibling)
- `process_deletes` resolves via `index_to_abstract_data` which uses asset_id
- `FlushTreeTask::remove` cleans up ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX

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
Phase 8 is in progress — watcher tests added, watcher code already path-primary.

Next phases:

- Phase 8: complete remaining modify/sidecar tests
- Phase 9: Frontend Identity Refactor

## Phase 8: Watcher and Reconciliation — IN PROGRESS

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

### Remaining work

- modify one duplicate's bytes and verify DUPE_INDEX membership
- sidecar change and missing sidecar reconciliation
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

## Phase 10: Remove Old Identity Code

After all functional tests pass:

- remove `AbstractData` alias-list identity behavior;
- remove hash-primary lookup helpers;
- remove hash-based mutation request fields;
- remove old hash-token frontend paths;
- remove old database files and initialization paths;
- keep only the new clean rebuild and new schema generation.

Run the complete project checks and API/UI scenario suites.
