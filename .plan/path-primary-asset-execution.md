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

## Phase 4: Indexing and Duplicate Handling

Change `index_image` and `DeduplicateTask`:

- lookup by canonical path first;
- update the path asset if it already exists;
- otherwise allocate a new asset ID;
- update the hash group independently;
- never append a path to another asset record;
- never delete a path because its hash already exists.

Tests:

- indexing identical bytes at two paths creates two asset IDs;
- indexing either path again is idempotent;
- changing bytes at an existing path updates that asset’s hash group;
- deleting/replacing one path does not affect the other;
- a concurrent duplicate index does not create two assets for one path.

## Phase 5: Snapshot and Query Read Path

Change the backend read path in this order:

1. `ReducedData` stores `asset_id`.
2. Tree snapshots store asset IDs.
3. `get-data` resolves snapshot positions to asset IDs.
4. `locate` accepts asset IDs.
5. Album filtering excludes album assets from media grids.
6. Existing full scans may remain temporarily for non-identity filters.

Acceptance tests:

- two same-hash assets occupy two snapshot rows;
- both survive snapshot rebuild;
- each asset locates independently;
- pagination returns both rows;
- refreshing query state does not collapse them.

Do not add tag/date/type indexes in this phase unless a failing functional
test requires one for correctness.

## Phase 6: Serving, Tokens, and API Responses

Change identity-dependent API behavior:

- response asset identity becomes `asset_id`;
- original-file serving resolves `asset_id` to the current path;
- thumbnail serving may continue using the shared content hash;
- tokens bind to asset ID for originals and mutations;
- album covers reference an asset ID.

Negative tests:

- asset A’s token cannot serve asset B;
- unknown asset ID returns the correct error;
- moved asset serves from its new path;
- deleted asset cannot be served;
- shared thumbnail remains usable for a surviving duplicate.

Update OpenAPI annotations and generated API documentation with the new
request/response shapes.

## Phase 7: Move, Delete, Sidecars, and Album Operations

Migrate one mutation at a time:

1. `assign_album` accepts `asset_id` and moves exactly one path asset.
2. Delete/trash accepts `asset_id` and affects exactly one asset.
3. Sidecar move/delete follows the selected asset.
4. Shared thumbnail cleanup consults `DUPE_INDEX`.
5. Directory moves update descendant asset paths and album records.
6. Album deletion recursively handles child album/file assets according to
   the filesystem design.

For every mutation test:

- success response;
- physical file state;
- sidecar state;
- asset record state;
- album query state;
- duplicate-group state;
- thumbnail state;
- stale/unknown ID negative path.

Only after these tests pass should the old hash-plus-alias mutation code be
removed.

## Phase 8: Watcher and Reconciliation

Change watcher/indexer events to begin with canonical path lookup:

- create: create/update one path asset;
- modify: reconcile the existing path asset and hash membership;
- remove: remove/reconcile one path asset;
- rename: handle as an explicit path transition or remove/create pair;
- uncertain or dropped events: run the existing filesystem rebuild path.

Tests must cover:

- create identical file at a new path;
- modify one duplicate’s bytes;
- remove one duplicate;
- rename one duplicate;
- stale path after external deletion;
- sidecar change and missing sidecar;
- partial/canceled scan does not delete outside-scope assets.

Only introduce a durable operation journal if these failure tests demonstrate
that rebuild/reconciliation is insufficient and the operation protocol
requires one.

## Phase 9: Frontend Identity Refactor

Replace hash identity in this order:

- `dataStore` maps by asset ID;
- worker payloads and row data use asset ID;
- routes and view navigation use asset ID;
- original-image cache uses asset ID;
- shared thumbnail cache may use hash;
- token persistence uses asset ID;
- menus, downloads, covers, metadata, move, and delete use asset ID;
- direct alias/path presentation becomes direct canonical asset path.

Playwright tests must verify duplicate rendering and independent selection,
move, delete, refresh, pagination, and original serving.

## Phase 10: Remove Old Identity Code

After all functional tests pass:

- remove `AbstractData` alias-list identity behavior;
- remove hash-primary lookup helpers;
- remove hash-based mutation request fields;
- remove old hash-token frontend paths;
- remove old database files and initialization paths;
- keep only the new clean rebuild and new schema generation.

Run the complete project checks and API/UI scenario suites.
