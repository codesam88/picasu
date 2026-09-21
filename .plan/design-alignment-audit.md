---
status: done
type: chore
priority: high
area: full-stack
---

# Path-Primary Design Alignment Audit

## Findings

### Backend: Optional asset_id fields that should be required — RESOLVED

All fields below are now required (no `Option`, no `None` default). Resolved in commits `39cc2987`, `7ef17352`.

### Backend: Hash-based lookup functions — RESOLVED

All hash-based lookup functions below have been removed or converted to asset-ID lookups. Resolved in commits
`2ef81723`, `7ef17352`.

### Backend: Legacy API fields — RESOLVED

`rotate-image` and `regenerate-thumbnail` now use `asset_id`. `get/test/record` uses asset_id path. Resolved in
commit `2ef81723`.

### Backend: Backward-compat code paths — RESOLVED

All fallback paths removed. `asset_id_to_abstract_data` reads from `DATA_TABLE` only. `ReducedData.asset_id` is
required. `DeleteList` uses `asset_ids`. Resolved in commits `39cc2987`, `7ef17352`, `1e8f55f1`.

### Frontend: Dual maps and hash-based identity — RESOLVED

`hashMapData` removed; `assetIdMapData` is the sole identity map. `getAssetIndexDataFromRoute` uses `assetIdMapData`.
View page uses `route.params.assetId`. Resolved in commits `69511da2`, `bd5b6454`.

## Completed Work

### Commit 1: `39cc2987` — Make asset_id required in response types

- `ClaimsHash.asset_id`: `Option` → required `ArrayString<64>`
- `DatabaseTimestamp.asset_id`: `Option` → required `ArrayString<64>`
- `DataBaseTimestampReturn.asset_id`: `Option` → required `String`
- `ReducedData.asset_id`: `Option` → required `ArrayString<64>`
- Remove `DataBaseTimestampReturn::new()` (dead code)
- Remove `abstract_data_to_database_timestamp_return()` (dead code)
- Remove `index_to_hash()` and `hash_to_abstract_data()` (dead code)
- Remove `build_from_data_table()` legacy fallback path
- `asset_id_to_abstract_data`: remove ASSET_BY_ID fallback
- `index_to_asset_id`: return `ArrayString` directly, not `Option`
- All mutation endpoints require asset_id for identity
- Album cover references use asset_id exclusively

### Commit 2: `2ef81723` — Convert rotate/regenerate to use asset_id

- `RotateImageRequest`: `hash` → `asset_id`
- `RegenerateThumbnailForm`: `hash` → `asset_id`
- Rename `lookup_abstract_data_by_hash` → `lookup_abstract_data_by_asset_id`
- Remove hash fallback in lookup function
- Both endpoints now identify images by asset_id exclusively

### Commit 3: `7ef17352` — Make AssignAlbumData.asset_id required

- `AssignAlbumData`: remove `hash` field, make `asset_id` required
- Remove `resolve_asset_id_from_hash()` and `move_hash_into_album()`
- Remove `move_item_into_album()` legacy hash-based path
- `move_asset_into_album`: update `DATA_TABLE` with new album and path
- `asset_record_to_abstract_data`: use content hash for media (thumbnail
  paths), asset_id for albums
- Update all test scenarios to use `asset_id` for `assign_album` calls
- Add `asset_id_as` to test scenarios that need asset_id discovery
- `discover_photo_hash` returns content hash for backward compatibility
  with test assertions that check `abstractData.id`

## Remaining Work

All items from the original findings have been resolved. `get_test_probe.rs` was updated to use asset_id only
(commit `6746adc3`).

### Completed in this audit

#### Commit: Backend delete — replace aliasList/index deletion with asset-ID deletion

- `DeleteList`: `delete_list` + `alias_list` → `asset_ids`
- `process_deletes`: look up by `asset_id` in `DATA_TABLE`, delete file +
  sidecar, preserve shared thumbnails via `DUPE_INDEX`
- Remove `index_to_abstract_data` from `transitor.rs` (dead code)
- Remove 7 obsolete alias-level deletion scenarios
- Update 13 scenarios to use `asset_ids` instead of indices/aliasList
- Add `delete_by_asset_id` scenario for new API coverage

#### Commit: Frontend Slice 1 — replace hashMapData with assetIdMapData

- Remove `hashMapData` from `dataStore.ts`, use `assetIdMapData` exclusively
- `fromDataWorker`: store by `assetId` only, not content hash
- `getHashIndexDataFromRoute` → `getAssetIndexDataFromRoute`, use `assetIdMapData`
- `ViewPage`: use `assetIdMapData` for index lookup, `assetId` for navigation
- `refreshAlbumMetadata`, `createAlbums`: use `assetIdMapData`
- `ItemRegenerateThumbnailByFrame`: use `assetId` directly from route
- `rotate.ts`: send `assetId` to backend `rotate-image` endpoint
- `ItemSetAsCover`: send `coverAssetId` to backend `set_album_cover` endpoint

#### Commit: Frontend Slice 2 — rename route param :hash to :assetId

- `createRoute`, `routes`, `shareRoute`: change `:hash` to `:assetId` in paths
- All View/Display/Metadata components use `route.params.assetId`
- `usePrefetch`: use `route.params.assetId` for locate

#### Commit: Frontend Slice 3 — rename hash props to assetId

- `SingleMenu`, `ShareMenu`, `AlbumMenu`: rename `hash` prop to `assetId`
- `ItemFindInTimeline`, `ItemViewOriginalFile`: rename `hash` prop to `assetId`
- `Display`, `DisplayDesktop`, `DisplayMobile`: rename `hash` prop to `assetId`
- `MetadataContent`, `ViewPageMetadata`: rename `hash` prop to `assetId`
- Compressed thumbnail URLs still use content hash (`abstractData.id`)

#### Commit: Fix remaining hash-based route guard and delete request

- `routes.ts`: change `to.params.hash` to `to.params.assetId` in title guard
- `ItemPermanentlyDelete`: send `assetIds` instead of `deleteList`/`aliasList`
- Fix `dup_delete_one_leaves_other` scenario: restore `duplicate_of` directive

#### Commit: Remove frontend backward-compatibility fallback

- `schemas.ts`: `assetId` required in `databaseTimestampSchema` (was optional)
- `types.ts`: `assetId` required in `EnrichedUnifiedData` and `SlicedData`
- `getter.ts`: `getSrc` throws when `assetId` missing for original URLs
- `workerApi.ts`: `assetId` required in `ProcessSmallImagePayload`/`ProcessImagePayload`
- `toImgWorker.ts`: remove `assetId ?? hash` fallback, use `assetId` directly
- `fromDataWorker.ts`: remove `if (assetId !== undefined)` guards
- `toDataWorker.ts`: use `EnrichedUnifiedData` type, import it
- `createData.ts`: `enrichWithThumbhash` requires `assetId` in input
- Remove dead `assetId === undefined` guards in components
- Update tests: assert required-ID error, remove fallback expectations

## Remaining Legitimate Hash Uses

Content hash is used only for:

1. Compressed thumbnail URLs (`/object/compressed/{hash[0:2]}/{hash}.jpg`)
2. `DisplayDatabaseVideo` video URL construction
3. `DUPE_INDEX` grouping (backend)
4. Album cover content hash for thumbnail path resolution
5. `ClaimsHash.hash` field for compressed image guard validation (GuardHash)
