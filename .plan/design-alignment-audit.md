---
status: done
type: chore
priority: high
area: full-stack
---

# Path-Primary Design Alignment Audit

## Findings

### Backend: Optional asset_id fields that should be required

| File                            | Field                              | Issue                             |
| ------------------------------- | ---------------------------------- | --------------------------------- |
| `router/auth.rs:84`             | `ClaimsHash.asset_id`              | `Option` with `None` default      |
| `model/response.rs:13`          | `DatabaseTimestamp.asset_id`       | `Option` with `None` default      |
| `model/response.rs:52`          | `DataBaseTimestampReturn.asset_id` | `Option`, `skip_serializing_if`   |
| `model/response.rs:127`         | `ReducedData.asset_id`             | `Option`                          |
| `router/put/assign_album.rs:44` | `AssignAlbumData.asset_id`         | `Option` with fallback resolution |

### Backend: Hash-based lookup functions still in use

| Function                       | Location                         | Callers                                |
| ------------------------------ | -------------------------------- | -------------------------------------- |
| `lookup_abstract_data_by_hash` | `storage/asset_store.rs:157`     | `rotate_image`, `regenerate_thumbnail` |
| `resolve_asset_id_from_hash`   | `router/put/assign_album.rs:150` | `move_hash_into_album`                 |
| `index_to_hash`                | `process/transitor.rs:27`        | dead code (`#[allow(dead_code)]`)      |
| `hash_to_abstract_data`        | `process/transitor.rs:44`        | dead code (`#[allow(dead_code)]`)      |

### Backend: Legacy API fields

| Endpoint                                   | Field           | Issue                |
| ------------------------------------------ | --------------- | -------------------- |
| `PUT /put/rotate-image`                    | `hash: String`  | Should be `asset_id` |
| `PUT /put/regenerate-thumbnail-with-frame` | `hash: String`  | Should be `asset_id` |
| `GET /get/test/record/{hash}`              | hash path param | Should be asset_id   |

### Backend: Backward-compat code paths

| Location                | Code                                                             | Issue                      |
| ----------------------- | ---------------------------------------------------------------- | -------------------------- |
| `transitor.rs:58-72`    | `asset_id_to_abstract_data` falls back to ASSET_BY_ID            | Should only use DATA_TABLE |
| `transitor.rs:87`       | `display_id = content_hash.unwrap_or(asset_id)`                  | Should use asset_id        |
| `transitor.rs:188`      | `asset_id: None` in `abstract_data_to_database_timestamp_return` | Should require asset_id    |
| `update_tree.rs:99-116` | `build_from_data_table`                                          | Legacy fallback path       |
| `delete.rs:255`         | "Legacy path: aliasList not provided"                            | Should require aliasList   |

### Frontend: Dual maps and hash-based identity

| Location           | Code                                         | Issue                     |
| ------------------ | -------------------------------------------- | ------------------------- |
| `dataStore.ts:8`   | `hashMapData: Map<string, number>`           | Dual map for URL routing  |
| `getter.ts:24`     | `getHashIndexDataFromRoute` uses hashMapData | Should use assetIdMapData |
| `ViewPage.vue:116` | `route.params.hash`                          | Route uses hash param     |
| `routes.ts:90`     | `params: { hash: route.params.hash }`        | Route uses hash param     |

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

The following items from the original findings are NOT addressed in this
audit and should be tracked separately:

### Backend

- `get_test_probe.rs` — still uses hash-based DUPE_INDEX resolution

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

## Remaining Legitimate Hash Uses

Content hash is used only for:

1. Compressed thumbnail URLs (`/object/compressed/{hash[0:2]}/{hash}.jpg`)
2. `DisplayDatabaseVideo` video URL construction
3. `DUPE_INDEX` grouping (backend)
4. Album cover content hash for thumbnail path resolution
