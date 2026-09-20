---
status: done
type: feature
priority: high
area: frontend
---

# Frontend Identity Migration — Correct Hash-Primary to AssetId-Primary

## Problem

The frontend identity migration (Phase 9) is incomplete. While `assetId` has been
added to the data pipeline (schema, worker, dataStore, getSrc), many parts of the
frontend still use content hash as the primary identity:

- `tokenStore.hashTokenMap` is keyed by `data.id` (content hash), not assetId
- `serviceWorker` extracts hash from URL filename for token lookup — fails for
  original URLs that now contain assetId
- `assignAlbum()` sends `hash` to backend, not `asset_id`
- `editStore` tracks rotation/regeneration by hash
- `toImgWorker` blob cache keys and `getSrc` calls use hash
- `ItemRegenerateThumbnailByFrame` uses route hash param

## Constraints

- No compatibility fallback identity for originals, tokens, or mutations
- `assetId` must be the frontend identity for: original URLs, original caches,
  token storage/lookup, selection, routes, move/delete/edit requests, worker payloads
- Content hash may remain only for explicitly shared compressed thumbnail URLs/cache

## Completed Commits

### Commit 1: token storage/service-worker lookup by asset ID — `da81f11d`

- Rename `hashTokenMap` → `assetTokenMap` in tokenStore, keyed by assetId
- Rename db.ts functions: `storeHashToken`/`getHashToken`/`deleteHashToken` →
  `storeAssetToken`/`getAssetToken`/`deleteAssetToken`
- Service worker extracts assetId from original URL filenames for token lookup
- `fromDataWorker` stores tokens by assetId for media items, by content hash
  for album covers
- SmallImageContainer, Display, download components use assetId for token lookup
- Add `tokenStore.test.ts`: 4 tests for separate same-hash asset token entries

### Commit 2: mutation APIs and worker payloads use asset ID — `43e9fa3f`

- `assignAlbum()` sends `assetId` field in request body (backend supports it)
- `AssignAlbumModal` passes `item.assetId` to `assignAlbum()` instead of `item.id`
- `workerApi`: add optional `assetId` to `ProcessSmallImagePayload` and
  `ProcessImagePayload`
- `toImgWorker`: use `assetId` for blob cache keys when available
- `ItemRegenerateThumbnailByFrame`: resolve assetId from data store
- SmallImageContainer and Display pass assetId to worker payloads
- Add `editStore.test.ts`: 2 tests for independent asset tracking

### Commit 3: remove hash-identity fallback code — `59e82a5c`

- `fromDataWorker`: remove content-hash token fallback; warn when media item
  has no assetId
- SmallImageContainer, Display, download components: require assetId for media
  items instead of falling back to content hash
- `AssignAlbumModal`: require assetId for move operations
- `ItemRegenerateThumbnailByFrame`: require assetId for regeneration
- Content hash remains only for album covers (compressed thumbnails)

### Commit 4: Playwright scenarios for duplicate identity — `d89d9fe7`

- `duplicate-move-independently`: move one duplicate to album, other stays
- `duplicate-refresh-preserves-both`: page refresh preserves both duplicates
- `duplicate-pagination`: pagination loads duplicate items across batches
- `duplicate-original-serving`: original file serving works for each duplicate
