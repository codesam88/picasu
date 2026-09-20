---
status: in-progress
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

## Execution Plan

1. Make `asset_id` required in `ClaimsHash`, `DatabaseTimestamp`, `DataBaseTimestampReturn`, `ReducedData`
2. Make `asset_id` required in `AssignAlbumData`, remove `resolve_asset_id_from_hash`
3. Convert `rotate_image` and `regenerate_thumbnail` to use `asset_id`
4. Remove dead code (`index_to_hash`, `hash_to_abstract_data`, `build_from_data_table`)
5. Remove `lookup_abstract_data_by_hash` — replace callers with asset_id lookups
6. Clean `transitor.rs` backward-compat paths
7. Frontend: remove `hashMapData` dual map, use `assetIdMapData` for all lookups
