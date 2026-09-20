---
status: open
type: feature
priority: high
area: backend
---

# Path-Primary Asset Index

## Problem

The current data model uses the content hash as the primary asset identity.
One database record can contain several filesystem aliases. This couples
deduplication, filesystem identity, album membership, thumbnail ownership,
and API presentation.

That causes observable ambiguity:

- Identical files in the same album collapse into one API/UI item because the
  API returns one record per hash rather than one record per file.
- Moving or deleting one alias requires special multi-alias behavior and can
  leave the record visible through another alias.
- Filesystem paths and sidecars are first-order objects, but the database
  represents them as secondary aliases.
- Hash-based metadata sharing is useful, but hash-based presentation is not
  aligned with the file-first filesystem model.

## Decided Design

Make each existing filesystem asset its own record. The canonical repository
path is the authoritative filesystem index key. Give the record a stable
opaque `asset_id` for API and frontend references where stable identity across
moves is useful.

Conceptual indexes:

```text
ASSETS_BY_PATH[canonical_path] -> {
    asset_id,
    path,
    type,
    blake_hash,
    metadata,
    trashed,
    ...
}

ASSETS_BY_ID[asset_id] -> canonical_path

HASH_INDEX[blake_hash] -> asset_id list
```

`ASSETS_BY_PATH` is the authoritative asset table. `ASSETS_BY_ID` is an
opaque API lookup convenience, not a second source of truth. Filesystem
events and indexing begin with a path lookup; normal API operations begin
with an asset ID lookup. A move updates both path identity and the stable ID
mapping in one journaled/index transaction.

An alternative implementation may store the full record under `asset_id`
and maintain `PATH_INDEX` as the authoritative unique path constraint, but it
must preserve the same semantics: one record per physical path, path lookup,
stable API identity, and rebuildable indexes.

```text
ASSET_BY_ID[asset_id] -> {
    path,
    type,
    blake_hash,
    metadata,
    trashed,
    ...
}
```

Albums use canonical directory paths as their filesystem identity and retain
their own stable IDs. Existing path-range/filter indexes remain part of the
design: album, tag, type, date, trash, and other query indexes map categories
or ranges to asset IDs and are maintained when an asset path or indexed
metadata changes.

The API identifies assets by `asset_id`. Paths remain server-side data and
filesystem lookup keys rather than arbitrary strings transported through the
normal API signal path. API responses may include the path as display/data
metadata, but mutations should target `asset_id` and the server-resolved
current path.

Content deduplication remains available for:

- duplicate detection and duplicate-management views;
- sharing thumbnails or other content-derived objects;
- reverse lookup of all assets with the same content.

Deduplication no longer merges independent filesystem files into one
presented asset record.

## Why This Design

### Benefits

- Every physical file has one independent API/UI identity.
- Identical files are displayed independently.
- Moving one file cannot implicitly move another file's asset record.
- Delete, trash, sidecar, and album membership operations become file-local.
- Stable asset IDs survive path moves.
- Hash lookup remains available for duplicate tracking and shared thumbnails.
- Existing category/query indexes can continue to return asset IDs.

### Costs

- Existing hash-keyed records must be split into one asset per alias.
- Because generated state is rebuildable from the repository, prefer a
  versioned index rebuild from the filesystem over treating the existing DB as
  authoritative migration input. A compatibility migration may still be
  useful for preserving user metadata and avoiding a long cold rebuild.
- Asset records may duplicate metadata that was previously shared.
- Path, asset, and query indexes must be updated atomically on move/delete.
- Metadata ownership must be explicit: per-file metadata versus content-level
  metadata.
- Re-indexing and filesystem reconciliation become more important because path
  entries are primary records.
- The migration and API changes touch many lookup and response paths.

## Existing Indexes and Required Revision

The implementation audit found that the current system has fewer durable
indexes than the earlier design wording implied. `DATA_TABLE` in
`backend/src/storage/db.rs` is the authoritative hash-keyed table. The other
Redb stores are primarily tree snapshots, query snapshots, and expiry state;
they are not durable path/category indexes. The implementation should
therefore introduce the required indexes deliberately rather than assume that
existing materialized indexes can simply be retargeted.

Existing structures to inventory and revise, rather than replace blindly:

- `DATA_TABLE` and `TREE`: currently hash-keyed records and in-memory tree
  construction.
- `FlushTreeTask` and all insert/remove/update callers: primary-key changes and
  multi-index updates.
- `DeduplicateTask`: stop merging aliases into the existing asset; create or
  update the independent path asset and update `HASH_INDEX`.
- Album/path handling in `process/dir_album.rs`: preserve canonical directory
  identity and parent/child album relationships.
- Expression filtering and prefetch/query caches: return asset IDs and use
  maintained path/category indexes instead of relying on alias membership.
- Tag, type, date, trash, favorite, archived, and other filter indexes or
  derived query structures: map to asset IDs and update on metadata changes.
- `UpdateTreeTask`, album self-update, and cache invalidation: handle path
  moves and independently visible assets.
- Watcher and album-index flows: create, update, and remove path assets when
  filesystem events occur.
- Delete/trash and sidecar handling: operate on one asset path at a time;
  remove shared thumbnails only when no asset references the content hash.
- Image/video serving and API token generation: resolve `asset_id` to the
  current path and content hash.
- Frontend response models and mutations: use stable asset IDs instead of
  treating a hash or surfaced alias path as the item identity.
- Test fixtures and scenarios: replace assumptions that one hash equals one
  visible image, while preserving explicit duplicate-group assertions.

Additional concrete identity consumers found during the audit:

- `storage/cache.rs`, `model/response.rs`, and `process/transitor.rs` store and
  resolve snapshot positions as hashes;
- `router/get/get_img.rs` resolves originals by hash and first alias;
- `router/auth.rs` creates and validates hash-bound tokens;
- `router/delete.rs`, `router/put/assign_album.rs`, edit routes, rotation, and
  thumbnail regeneration accept hashes or snapshot indexes;
- `frontend/src/store/dataStore.ts`, workers, token storage, routes, display,
  downloads, covers, and metadata views assume hash or `alias[0]` identity;
- `model/album.rs` stores cover references as hashes.

These are required migration sites, not optional compatibility cleanup.

## Current Database and Table Requirements

The existing Redb layout must be versioned explicitly. The current value
serialization version is not a database migration mechanism: changing the
`AbstractData` value does not split one hash record into several path assets or
create new tables.

The minimum durable structures are:

```text
ASSET_BY_ID[asset_id] -> AssetRecord { path, type, hash, metadata, ... }
PATH_INDEX[canonical_path] -> asset_id
HASH_INDEX[(hash, asset_id)] -> unit
ALBUM_INDEX[(album_id, asset_id)] -> unit
DATE_INDEX[(timestamp, asset_id)] -> unit
TYPE_INDEX[(type, asset_id)] -> unit
TRASH_INDEX[(trashed, asset_id)] -> unit
TAG_INDEX[(tag, asset_id)] -> unit
DIR_ALBUM_BY_PATH[canonical_dir_path] -> album_id
DIR_ALBUM_BY_ID[album_id] -> canonical_dir_path
```

Composite `(hash, asset_id)` membership keys are preferable to one large
serialized list per hash, because adding or removing one duplicate should not
rewrite a potentially large duplicate group.

The canonical path index is the unique filesystem constraint. The asset-ID
table is the normal API lookup. Both must be updated in the same durable
transaction or through replayable journal state. Path normalization must
define relative/absolute representation, separators, case sensitivity,
Unicode normalization, symlink policy, and containment under `IMAGE_PATH`.

## Database Engine Status

Engine selection is intentionally deferred to the idea-level task
`.plan/database-engine-evaluation.md`. The asset design must not currently
assume that Redb requires bespoke query planning, recovery, or journaling code;
those are questions to verify against the actual workload and documented
engine behavior.

Backward compatibility is not a goal for this redesign. A new database
generation may be rebuilt from the filesystem after the engine and schema are
selected, with explicit rules for preserving sidecars, user metadata, and
shared derived objects.

## Watcher, Indexer, and Recovery Requirements

The current watcher and album indexer are alias/hash based and have several
performance and correctness hazards:

- watcher removal scans the in-memory tree instead of doing a path lookup;
- watcher modify events index by hash, so a changed path can merge into an
  unrelated duplicate;
- watcher handling has no explicit overflow, rename-pair, or dropped-event
  reconciliation path;
- album indexing creates one task per file and performs stale-alias cleanup
  after partial/canceled scans;
- stale cleanup can affect aliases outside the requested scan root;
- `DeduplicateTask` prunes missing aliases while indexing, conflating
  reconciliation with discovery;
- hash-keyed in-progress guards can suppress independent same-content paths;
- directory moves scan the complete data table and rewrite alias strings;
- album cache updates occur separately from filesystem and database changes.

Required path-primary behavior:

1. Canonicalize the path at watcher/indexer boundaries.
2. Resolve or create exactly one asset for that path.
3. Use path identity for create, modify, remove, and stale reconciliation.
4. Use hash only to update duplicate membership and shared derived objects.
5. Bound indexer concurrency and avoid treating a partial scan as complete.
6. Reconcile after watcher overflow, cancellation, or uncertain rename events.
7. Journal move/delete/sidecar intent and progress, then recover at startup.
8. Do not report success until filesystem and required index state are
   durable.

There is currently no operation journal/recovery table. This is a prerequisite
for making path/index updates safe across crashes; it should not be deferred
until after the identity migration.

## Snapshot, Query, and Cache Requirements

Current `UpdateTreeTask` and prefetch code rebuild and filter the complete
in-memory tree, while persisted snapshots store hashes. At 10 million assets,
the path-primary implementation must avoid preserving this as the only query
strategy.

Required changes:

- snapshots store `asset_id`, not hash;
- `get-data` resolves snapshot position -> asset ID -> asset record;
- `locate` accepts asset ID; hash-only locate is ambiguous once duplicates
  exist and must be rejected or explicitly return a duplicate group;
- indexed predicates produce candidate asset IDs before residual filtering;
- album listings use canonical path-prefix ranges or `ALBUM_INDEX`;
- date ordering uses an ordered `(date, asset_id)` index;
- tag/type/trash/favorite/archived filters have maintained candidate indexes
  or an explicitly bounded fallback scan;
- query cache keys include asset-index/schema generation and share context;
- old hash snapshots and row caches cannot be reused as asset-ID snapshots;
- locate and row-position maps are keyed by snapshot generation and asset ID.

The existing version counter and asynchronous cache invalidation paths are not
enough by themselves. A committed index generation must be published after a
mutation so queries cannot combine old snapshots with new path/category
indexes.

## API, Token, and Frontend Requirements

Normal mutations must accept `asset_id`, not a hash plus selected alias path.
Snapshot indexes may remain as optimistic-concurrency inputs, but the server
must resolve and validate the asset ID before mutating.

Required API changes include:

- asset-ID response identity and asset-ID pagination;
- asset-ID original-file serving and authorization claims;
- asset-ID delete, move, rotate, tag, rating, description, flags, and
  thumbnail-regeneration requests;
- album covers referencing an asset ID, with a separate content-hash thumbnail
  reference if thumbnails are shared;
- explicit compatibility behavior for old hash-based URLs, since a hash no
  longer uniquely selects an asset.

Frontend migration sites include the hash map in `dataStore`, worker payloads,
route parameters, display/navigation, image cache keys, token IndexedDB,
downloads, covers, and all uses of `alias[0]`.

Original-file caches must be keyed by asset ID. Shared compressed-thumbnail
caches may be keyed by hash only when authorization and rendering semantics
permit sharing.

## Sidecar and Thumbnail Ownership

The current alias model hides sidecar ownership and treats the first alias as
the source for metadata operations. The new model must define:

- one owning asset path per sidecar;
- move/delete sidecar behavior and failure recovery;
- whether external sidecar changes trigger indexing;
- which metadata is per-asset versus content-shared;
- thumbnail reference counting through `HASH_INDEX` or a dedicated content
  reference table.

Deleting one asset must not remove a shared thumbnail while another asset uses
the same hash. Deleting the final reference may remove it after durable
reconciliation.

The change must also preserve the design document's filesystem-operation
boundary:

- journal move/delete intent and progress;
- do not report success before required filesystem changes are durable;
- leave partial operations visible for startup recovery and reconciliation;
- move sidecars with their owning path asset;
- never let indexing or duplicate grouping delete or move a file.

## Migration Strategy

1. Define canonical relative path rules and stable `asset_id` format.
2. Add versioned asset/path/hash tables without changing existing behavior.
3. Build a migration that splits every existing multi-alias record into one
   asset per live alias and populates secondary indexes.
4. Add consistency checks for duplicate paths, missing files, stale records,
   and hash-index references.
5. Migrate read paths and API responses to asset IDs.
6. Migrate write paths: index, watcher, move, delete, trash, sidecar, and
   metadata updates.
7. Remove alias-based identity assumptions after old records and callers are
   retired.

The migration must preserve the existing content hash and thumbnail data.
Where multiple aliases shared metadata, the migration must define whether
metadata is copied to each asset or split into content-level and asset-level
fields.

## Performance Impact

For approximately 10 million assets, B-tree lookups remain `O(log N)`:

- asset ID lookup: one lookup;
- path lookup: one path-index lookup;
- hash lookup: one hash-index lookup plus `O(K)` duplicate results;
- asset move/delete: updates to a small number of indexes;
- album listing: `O(log N + K)` when backed by a path-prefix range index.

The main risks are database size, Redb's single-writer throughput, cache
pressure, and full re-index duration. Thumbnail storage is expected to be
larger than the metadata indexes and can continue using content-addressed
hashes where safe.

## Effort Estimate

This is a large backend/data-model change rather than a local refactor.

- Schema, canonical paths, identity, and journal design: 2–4 days
- Index/migration/rebuild prototype and consistency checks: 4–8 days
- Snapshot/query/index-generation migration: 4–8 days
- Core indexing, watcher, move, delete, and sidecar recovery: 6–12 days
- API, tokens, frontend stores/routes/workers, and cache migration: 5–10 days
- Scenario/UI coverage, performance testing, migration tooling, and cleanup:
  5–10 days

Estimated total: **26–52 engineering days**, depending on compatibility
requirements, metadata migration policy, and how much query indexing is
materialized instead of derived from snapshots.

## Open Questions

- Is `asset_id` allocated randomly or deterministically from the initial path?
  Allocated IDs are stable across moves; path-derived IDs are simpler but
  change on rename.
- Which metadata is asset-specific versus content-shared?
- Should identical assets share thumbnails automatically, or only after an
  explicit duplicate decision?
- Are duplicate groups represented in `HASH_INDEX` only, or persisted as a
  dedicated duplicate-management table?
- Which existing category indexes are materialized in Redb versus derived in
  memory/query caches? The current audit indicates most filtering is a full
  in-memory tree scan rather than a durable category index.
- What compatibility period is required for hash/path-based API requests?

## Test Strategy and Acceptance Coverage

Follow `docs/test-strategy.md` and keep tests at the layer where they provide
useful coverage:

### Unit tests

Test pure logic with non-trivial branches:

- canonical path normalization and path-prefix range boundaries;
- stable asset ID generation/allocation rules;
- path/hash index update calculations;
- duplicate-group membership and cleanup decisions;
- migration/rebuild conflict handling.

Do not add tests for Redb's primitive read/write behavior.

### Integration tests

Use real Redb and temporary filesystem state for interactions that HTTP
scenarios cannot isolate well:

- one path asset per physical file after indexing identical bytes;
- atomic move/delete index updates;
- stale path and watcher reconciliation;
- crash/recovery or journal replay;
- shared thumbnail retention until the final asset reference is gone.

### API scenarios

Use `backend/tests/scenarios/*.yaml` for observable API and filesystem
contracts:

- identical files in one album return two image elements with distinct
  `asset_id` values;
- identical files in different albums remain independently movable;
- moving one duplicate leaves the other path and API item untouched;
- deleting one duplicate leaves the other file, record, and shared thumbnail;
- deleting the final duplicate removes generated state when policy allows;
- path, tag, trash, album, and duplicate queries return the expected asset
  IDs after moves;
- API responses and mutations no longer require hashes or surfaced alias
  paths as identity.

### UI scenarios

Add Playwright coverage for the reported user-visible behavior:

- upload an identical file into an album and assert two grid elements;
- refresh the album and assert both remain visible;
- move one duplicate and assert only that item leaves the source album;
- verify the other duplicate remains visible and selectable;
- delete one duplicate and verify the remaining item and thumbnail.

These UI scenarios are required because an API scenario alone cannot detect
frontend deduplication, stale stores, or rendering/pagination problems.

### Performance tests

Keep large-scale benchmarks separate from ordinary API-E2E scenarios. Measure
path lookup, asset lookup, hash-group lookup, album range listing, batched
indexing, move/delete write throughput, rebuild duration, and database size
at representative scales including 10 million assets.

## Initial Validation

Before implementation, add or run focused scenarios that establish current
behavior for:

- identical upload into the same album;
- identical files in different albums;
- moving one duplicate while the other remains;
- deleting one duplicate and then the last duplicate;
- album and tag query results after path moves;
- thumbnail reuse and cleanup when assets share a hash.

- frontend rendering of duplicate assets after upload and refresh;
- filesystem and DB state after moving a renamed duplicate away and back;
- recovery state after interrupted move/delete operations.

These scenarios should become migration acceptance tests, with expected
results updated to the path-primary model only after the design decisions
above are settled.

## Review Status

Reviewed against `docs/design.md` and `docs/test-strategy.md` on 2026-09-20.
The design document already requires distinct filesystem items, rebuildable
index state, sidecar pairing, non-destructive indexing, and explicit duplicate
cleanup. The test strategy requires unit, real-Redb integration, API scenario,
and Playwright coverage at their respective boundaries. The implementation
audit found that path indexes, journal/recovery, asset-ID tokens, and most
query indexes do not currently exist and must be treated as first-class scope.
