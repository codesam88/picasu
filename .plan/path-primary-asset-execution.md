---
status: open
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

## Phase 1: Test Fixtures and Scenario Helpers

Build deterministic test setup before changing identity code.

### Backend fixture helpers

Add a scenario fixture that creates two byte-identical valid media files at
different paths before indexing. It should support:

```yaml
given:
  - photo: /fixtures/source/photo.jpg
    id_as: $source
  - duplicate_of:
      source: /fixtures/source/photo.jpg
      destination: /fixtures/album/copy.jpg
```

The helper must copy bytes, not regenerate a similar image. Validate that both
files exist before the index operation.

Add reusable assertion/capture support as needed for:

- array length;
- distinct asset IDs;
- locating an asset by asset ID;
- asserting a path appears exactly once;
- asserting a shared hash group contains expected IDs.

Do not add production behavior in this phase.

### Required red scenarios

Add API scenarios for:

- identical files in one album return two items;
- identical files in separate albums return separate items;
- moving one duplicate leaves the other unchanged;
- deleting one duplicate leaves the other and shared thumbnail;
- deleting the final duplicate removes generated state according to policy;
- stale path and unknown asset ID are rejected safely.

Add Playwright scenarios for:

- two identical files render as two grid items;
- refresh preserves both items;
- selecting/moving one leaves the other visible;
- deleting one leaves the other usable.

These tests may fail until later phases. They must fail for the intended
identity assertion, not because fixture setup is invalid.

## Phase 2: New Data Model and Empty Database

Introduce the new schema generation with no compatibility reader.

Create typed records containing:

- `asset_id`;
- `kind`;
- canonical path;
- file-derived information;
- asset-owned information;
- optional content hash.

Create the minimum stores:

```text
ASSET_BY_PATH[path] -> asset_id
ASSET_BY_ID[asset_id] -> asset record
DUPE_INDEX[hash] -> asset ID list
```

Albums are `kind = album` records in the same stores. Do not create separate
album identity tables.

Add unit tests for:

- kind validation;
- media versus album hash rules;
- canonical path normalization;
- path uniqueness;
- asset-ID allocation;
- duplicate-list insertion/removal.

Add a Redb/SQLite integration test only for the chosen store's transaction
wrapper and schema initialization; do not test the database engine itself.

## Phase 3: Clean Filesystem Rebuild

Implement a rebuild that starts from an empty new database:

1. Walk the image root.
2. Create one album asset per directory.
3. Create one media asset per valid file.
4. Read/match sidecars without transferring metadata between paths.
5. Read `.albuminfo` for album assets.
6. Compute hashes for media assets.
7. Populate `DUPE_INDEX` without merging records.
8. Validate path uniqueness and asset/index consistency.

Negative tests:

- unsupported file is ignored or reported according to existing policy;
- missing sidecar does not delete the media asset;
- malformed `.albuminfo` does not delete the album or its children;
- file/directory path collision is rejected;
- path outside the image root is rejected;
- duplicate index references a missing asset are detected.

Verify the rebuild creates exactly one asset per physical file and directory.

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
