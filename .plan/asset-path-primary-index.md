---
status: done
type: feature
priority: high
area: backend
---

# Path-Primary Asset Index

## Problem

The original data model used the content hash as the primary asset identity.
One database record could contain several filesystem aliases. This coupled
deduplication, filesystem identity, album membership, thumbnail ownership,
and API presentation.

That caused observable ambiguity:

- Identical files in the same album collapsed into one API/UI item because the
  API returned one record per hash rather than one record per file.
- Moving or deleting one alias required special multi-alias behavior and could
  leave the record visible through another alias.
- Filesystem paths and sidecars are first-order objects, but the database
  represented them as secondary aliases.
- Hash-based metadata sharing is useful, but hash-based presentation was not
  aligned with the file-first filesystem model.

## Decided Design

Make each existing filesystem asset its own record. The canonical repository
path is the authoritative filesystem index key. Give the record a stable
opaque `asset_id` for API and frontend references where stable identity across
moves is useful.

Conceptual records and indexes:

```text
ASSET_BY_PATH[canonical_path] -> { asset_id }
ASSET_BY_ID[asset_id] -> {
    canonical_path,
    kind,                 # image, video, or album
    blake_hash?,
    file_info,
    asset_info,
}
DUPE_INDEX[blake_hash] -> { asset_id1, asset_id2, ... }
```

`ASSET_BY_PATH` is the authoritative unique path constraint, not a second
copy of the asset record. `ASSET_BY_ID` is the normal API and mutation lookup.
The canonical path appears in both the path key and the asset record because
the record must resolve the current file without scanning the path index; this
is limited, intentional denormalization. Both entries must be updated and
consistency-checked together.

Albums are a kind of asset in the same path/ID namespace:

- `kind = album`;
- the canonical path must resolve to a directory;
- albums have no content hash or duplicate-group membership;
- `asset_info` may contain trash and presentation state;
- `.albuminfo` remains the file-backed source for optional album title,
  description, notes, and cover metadata;
- child albums are derived from directory parent paths rather than a second
  album identity database.

Existing path-range/filter indexes remain optional design choices: album, tag,
type, date, trash, and other query indexes map categories or ranges to asset
IDs and are maintained when an asset path or indexed metadata changes.

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

- The current generated index is replaced; there is no old-DB content
  migration or compatibility period.
- The new database is built from the filesystem and current sidecars.
- Asset records may duplicate metadata that was previously shared.
- Path, asset, and query indexes must be updated atomically on move/delete.
- Metadata ownership must be explicit: per-file metadata versus content-level
  metadata.
- Re-indexing and filesystem reconciliation become more important because path
  entries are primary records.
- The refactor and API changes touch many lookup and response paths.

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
  update the independent path asset and update `DUPE_INDEX`.
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

These are required refactor sites, not optional compatibility cleanup.

## Current Database and Table Requirements

The existing Redb layout must be versioned explicitly. The current value
serialization version is not a database migration mechanism: changing the
`AbstractData` value does not split one hash record into several path assets or
create new tables.

The minimum durable structures are:

```text
ASSET_BY_PATH[canonical_path] -> { asset_id }
ASSET_BY_ID[asset_id] -> {
    kind,                 # image, video, or album
    canonical_path,
    blake_hash?,
    file_info,
    asset_info,
}
DUPE_INDEX[hash] -> { asset_id1, asset_id2, ... }
```

Only these are identity/storage requirements. The asset/path/duplicate indexes
answer “which filesystem object is this?”, “where is it?”, and “which media
assets share this content?”. An asset with `kind = album` is identified by the
same path/ID tables and must resolve to a directory.

`DUPE_INDEX` is a dynamically maintained group list. With duplicates expected
to be uncommon, the normal value is a one- or two-entry list and a hash lookup
is one indexed read returning the group. New assets append to the group; path
asset deletion or hash changes remove the corresponding ID. Group membership
updates must be atomic with the asset update.

This trades per-duplicate-row storage for rewriting one group value on
membership changes. That is appropriate while groups are normally small. If
measurements find pathological large groups or high same-hash write
contention, a threshold-based representation for those groups can be added;
that is not the default design.

The following are **optional query-acceleration candidates**, not required
databases and not currently implemented:

- `ALBUM_INDEX[(album_id, asset_id)]` for direct album membership;
- `DATE_INDEX[(timestamp, asset_id)]` for timeline ordering;
- `TAG_INDEX[(tag, asset_id)]` for tag filtering;
- type/extension indexes;
- trash, favorite, or archived indexes.

Whether these are worthwhile depends on measured query workload and the
selected database engine. Low-selectivity boolean fields may be better handled
by snapshots or residual filtering rather than separate indexes. Do not add
one durable index per response filter without a concrete query requirement.

`ASSET_BY_PATH` is a narrow unique lookup/constraint, not a second copy of
the asset record. `ASSET_BY_ID` is the normal API and mutation lookup. The
canonical path appears in both the path key and the asset record because the
asset record must be able to resolve the current file without scanning the
path index; this is limited, intentional denormalization. The two values must
be updated together and consistency-checked.

Suggested ownership split:

- `file_info`: facts derived from the current filesystem object, such as
  canonical path, type, size, modified time, dimensions, and content hash;
- `asset_info`: application state not guaranteed by the file itself, such as
  trash state, generated flags, and other asset-level presentation state;
- sidecar-backed metadata should have an explicit policy rather than being
  silently treated as either category.

The extra lookup for a filesystem-originated path is intentional:
`path -> asset_id -> asset`. Normal API operations use `asset_id -> asset` in
one lookup. A path-index value containing the full record would reduce that
lookup but duplicate all mutable asset state and create more consistency
failure modes.

The duplicate-group representation is separate from asset identity. Asset
records remain independently addressable even when they share one hash.

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
correctness hazards:

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

There is currently no operation journal/recovery table. Whether this is
required, or whether filesystem-driven rebuild/reconciliation is sufficient,
is an explicit design decision for the replacement implementation.

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
- asset-ID-only URLs and requests; old hash-only URLs are not a required
  compatibility surface because a hash no longer uniquely selects an asset.

Frontend refactor sites include the hash map in `dataStore`, worker payloads,
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
- thumbnail reference counting through `DUPE_INDEX` or a dedicated content
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

## Replacement Implementation Strategy

1. Define canonical relative path rules and stable `asset_id` format.
2. Select the authoritative database engine and create a new schema
   generation.
3. Rebuild the new asset and album index from the filesystem and sidecars.
4. Populate path, hash, album, and any selected query indexes from that scan.
5. Replace read paths and API responses with asset IDs.
6. Replace write paths: index, watcher, move, delete, trash, sidecar, and
   metadata updates.
7. Rebuild disposable tree/query/expiry stores from the new authoritative
   database.
8. Remove the old hash/alias implementation and old generated database files.

No old `index_v5.redb` records need to be read or converted. The filesystem,
sidecars, and explicit design policy are the inputs to the new index. Any
metadata that cannot be recovered from those sources is intentionally outside
the compatibility scope.

## Open Questions

- Is `asset_id` allocated randomly or deterministically from the initial path?
  Allocated IDs are stable across moves; path-derived IDs are simpler but
  change on rename.
- Which metadata is asset-specific versus content-shared?
- Should identical assets share thumbnails automatically, or only after an
  explicit duplicate decision?
- Are duplicate groups represented in `DUPE_INDEX` only, or persisted as a
  dedicated duplicate-management table?
- Which existing category indexes are materialized in Redb versus derived in
  memory/query caches? The current audit indicates most filtering is a full
  in-memory tree scan rather than a durable category index.
- Which filesystem/sidecar metadata is considered authoritative during the
  clean rebuild?

## Test Strategy and Acceptance Coverage

Follow `docs/test-strategy.md` and keep tests at the layer where they provide
useful coverage:

### Unit tests

Test pure logic with non-trivial branches:

- canonical path normalization and path-prefix range boundaries;
- stable asset ID generation/allocation rules;
- path/hash index update calculations;
- duplicate-group membership and cleanup decisions;
- clean-rebuild conflict handling.

Do not add tests for Redb's primitive read/write behavior.

### Integration tests

Use real Redb and temporary filesystem state for interactions that HTTP
scenarios cannot isolate well:

- one path asset per physical file after indexing identical bytes;
- atomic move/delete index updates;
- stale path and watcher reconciliation;
- crash/reopen and filesystem reconciliation, if required by the selected
  operation protocol;
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

Current contract tests added before the replacement implementation:

- `backend/tests/scenarios/duplicate_files_are_independent_album_items.yaml`
  asserts that two identical uploads produce two visible album items and two
  physical files. It failed under the hash-primary implementation, which
  returned one merged media item plus the album discovery fixture.
- `frontend/tests/playwright/scenarios/duplicate-files-visible-independently.yaml`
  asserts that two identical uploads remain as two grid items after a fresh
  route load. It failed because the frontend received one merged
  duplicate item.

Independent duplicate move/delete scenarios are intentionally deferred until
the asset-ID response and mutation contract is fixed. Writing those scenarios
against the current `hash + alias` request shape would preserve the ambiguity
the replacement implementation is intended to remove.

These scenarios are acceptance tests for the replacement implementation. The
hash-primary failures were intentional evidence of behavior that the
new implementation had to change.

## Closure note (2026-09-23)

Implemented in full via `path-primary-asset-execution.md` (all phases done, commits `16467741` + `33354c27`),
followed by the names/docs/structure cleanup in `design-sweep-and-structure-collapse.md` (`0c9d4ea3` through
`65f16c95`). Gates at closure:
`just check` 0, `just test` 0 — backend 263, utils 24, vitest 65, playwright 34.

Open questions resolved by the implementation:

- **asset_id allocation:** random (`generate_random_hash`), not path-derived — stable across moves/renames; path→id is a
  separate `ASSET_BY_PATH` mapping.
- **asset-specific vs content-shared:** lean `AssetRecord` (id, path, kind, size, times, album) is asset-specific;
  content-shared state is `content_hash` + `DUPE_INDEX` group + the shared compressed thumbnail keyed by hash.
- **thumbnail sharing:** automatic while a `DUPE_INDEX` group has >1 member; removed with the last member
  (`remove_compressed_thumbnail` group check, pinned by `dup_delete_preserves_shared_thumbnail`).
- **duplicate representation:** `DUPE_INDEX` only — no dedicated duplicate-management table.
- **category indexes:** filtering is a full in-memory tree scan plus per-timestamp/count query-snapshot caches; no
  durable tag/category index (deferred, see execution plan Phase 14).
- **rebuild authority:** the filesystem is authoritative; `.albuminfo`/XMP sidecars enrich album metadata on write;
  `rebuild_from_filesystem` derives album membership from parent directories.

Initial Validation scenarios listed here as intentionally red are now green: `duplicate_files_are_independent_album_items`
and the Playwright `duplicate-files-*` suite pass in every full gate.
