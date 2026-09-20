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

- `delete.rs:255` — "Legacy path: aliasList not provided" still exists
- `get_test_probe.rs` — still uses hash-based DUPE_INDEX resolution

### Frontend

- `hashMapData` dual map still exists for URL routing
- Routes still use `:hash` param
- `getHashIndexDataFromRoute` uses hashMapData
