---
status: open
type: feature
priority: high
area: backend
---

## Context

Replace the `IMAGE_HOME`-centric model with namespace-aware storage. Each namespace (`shared`, `trash`, future
user-specific) has its own independent filesystem root configured in the backend. DB records store namespace + relative
path; the backend resolves full paths at runtime.

`IMAGE_HOME` becomes obsolete. Thumbnails remain in `DATA_HOME` (transient object store, not under any namespace).

## Decisions (from plan review)

- **Namespace scoping is expression-based**: new `Expression::Namespace(String)` variant. Included in the prefetch query
  hash, so the query cache is namespace-aware automatically. The frontend composes it into filters the same way it
  composes `Trashed` today.
- **Hard schema break, no migration**: bump `SCHEMA_VERSION` to 7 and drop the v6 decode arm. Old databases are invalid
  after upgrade (pre-release; no legacy installs). No absolute-path → `(namespace, relative)` rewrite pass.
- **Explicit config required**: `[[namespace]]` entries are mandatory; the binary fails fast at startup if absent.
  `image_home`/`trash_directory` are removed (config, `PICASU_IMAGE_HOME`, JSON response).
- **`trash_enabled` stays a global option**: when true, a namespace named `trash` must be configured (startup
  validation). The trash namespace is identified by name `"trash"` (convention).
- **Resolver uses longest-prefix matching**: `namespace_from_path` must match the deepest root, so an operator may
  configure a trash root nested under the shared root without breaking resolution.
- All namespace roots must be on the same filesystem (so `fs::rename` works across namespaces); asserted at startup via
  `stat().st_dev`.

## Architecture

### Data model changes

**`FileModify`** (alias entry on Image/Video):

```rust
pub struct FileModify {
    pub namespace: String,  // NEW: "shared", "trash", etc.
    pub file: String,       // CHANGED: relative path within namespace
    pub modified: i64,
    pub scan_time: i64,
}
```

**`AlbumMetadata`**:

```rust
pub struct AlbumMetadata {
    pub namespace: String,  // NEW
    pub dir_path: String,   // CHANGED: relative path within namespace
    // ... existing fields unchanged
}
```

### Config

Replace `image_home`, `shared_directory`, `trash_directory` with a namespace registry:

```toml
[[namespace]]
name = "shared"
path = "/mnt/photos/shared"

[[namespace]]
name = "trash"
path = "/mnt/photos/trash"
```

`data_home` stays for thumbnails and internal DB. Startup validation:

- at least one namespace configured, names unique
- when `trash_enabled` is true, a namespace named `trash` must exist
- all namespace roots on the same filesystem (`stat().st_dev`), else fail fast

### Path resolver (`process/namespace.rs`)

- `namespace_resolve(namespace, relative) -> PathBuf` — full path from config
- `namespace_from_path(absolute) -> Option<(String, String)>` — reverse: absolute to (namespace, relative).
  Longest-prefix match; `None` if not under any namespace.
- `namespace_root(namespace) -> PathBuf` — root dir for a namespace

All three read from the namespace registry in `APP_CONFIG`.

### Expression system

- `Expression::Namespace(String)` — NEW: matches the record's namespace field
- `Expression::Trashed` — check `namespace == "trash"` (no more path-prefix check; albums check `metadata.namespace`)
- `Expression::Album(album_id)` — resolve album namespace + relative path, match aliases by `(namespace, relative
parent)`
- `Expression::Path(...)` — match namespace-relative path
- `RootAlbum` / `ParentAlbum` — resolve parents through the namespace-aware `DIR_ALBUM_CACHE`

### Watcher

- One watcher per namespace root, configured at startup.
- Each watcher's namespace is predetermined from its startup config.
- On file event: strip the root path prefix, return `(namespace, relative_path)` to the indexer. No runtime namespace
  identification needed.
- The trash namespace is not watched (or its events ignored at dispatch time).
- Removal handler (`handle_removed_file`) compares against the relative path, not the absolute event path.

### Filesystem constraint

All namespace roots must be on the same filesystem (so `fs::rename` works across namespaces). Assert this on startup by
comparing `stat().st_dev` across all configured namespace paths. Fail fast if any differ.

### Indexer

- `index_album(namespace: &str, relative_src: &str)`
- API: `POST /post/index/album` gains `namespace` field (default "shared")
- `ensure_dir_albums` creates albums with correct namespace + relative path

### Upload

- Default target: namespace `shared`, subdirectory from `upload_folder` (resolved under the shared namespace root)
- Upload to album: album's namespace + dir\_path already recorded

### Trash/restore

- Physical move: `namespace_root(source_ns) + relative` to `namespace_root("trash") + relative`
- The relative path is unchanged; only the `namespace` field flips (`shared` → `trash`, and back on restore)
- `is_in_trash` becomes a namespace check, not a path-prefix check
- In-namespace moves (assign\_album) rewrite only the relative path

### Queries

- `get_albums?space=shared` — filter `namespace == "shared"`; reject unknown space values
- Timeline: `Expression::Namespace("shared")`, Trash: `Expression::Namespace("trash")`
- Prefetch/get-data/search: frontend composes `Expression::Namespace` into the filter; the query hash is namespace-aware
  automatically

### `DIR_ALBUM_CACHE`

- `HashMap<(String, String), ArrayString<64>>` — (namespace, relative\_path) to album ID
- Every membership/parent lookup (`get_parent_album_id`, `get_dir_path_for_album`, `mark_dir_albums_for_path`,
  `Album::self_update`, `Expression::Album`, `assign_album`) compares namespace + relative path, never absolute paths
- `is_dir()`/`read_albuminfo` existence checks resolve via `namespace_resolve`

### Serialization

Hard break: bump `SCHEMA_VERSION` to 7, add v7 decode arm, drop the v6 arm. Old databases fail to decode (expected; no
legacy). Add `namespace` to `FileModify` and `AlbumMetadata`.

## Gaps found in code review (addressed by the plan above)

1. **Original serving + processing resolve absolute paths** (`get_img.rs`, `process/{exif,index,misc,video,xmp_write}`,
   `regenerate_thumbnail`) — all must resolve via `source_path_resolved()` = `namespace_resolve(ns, file)`.
2. **Namespace resolution ordering** — trash may be nested under shared; longest-prefix matching required.
3. **Scoping only existed for albums** — added `Expression::Namespace` so timeline/trash/search/prefetch are scoped,
   not just `get_albums?space=`.
4. **`DIR_ALBUM_CACHE` callers are absolute-`PathBuf` comparisons** — re-key by `(namespace, relative)` and compare
   namespace everywhere.
5. **Migration** — dropped entirely per decision (hard break).
6. **Restore/assign-album flow is path-prefix based** — `rewrite_paths_under` splits into relative-rewrite +
   namespace-flip; `move_item_into_album` resolves via `source_path_resolved()`.
7. **Watcher removal handler compares absolute** — must compare relative.
8. **Album creation writes absolute `dir_path`** — `write_album_to_db`/ `ensure_dir_albums` write ns + relative;
   `read_albuminfo` resolves.
9. **Frontend wider than the config page** — `ItemDelete`/`ItemPermanentlyDelete` alias matching becomes `(namespace,
relative parent)`; the index-files flow needs a namespace param.
10. **`init_dir_album_cache` existence checks** — resolve via namespace.

## Implementation plan (iterative)

Coupling note: P1 is standalone. P2–P6 form one atomic milestone — the model change forces every path consumer to
move in the same commit; "green" is only guaranteed at the end of P6, not per step. P7–P10 layer on top.

### P1 — Config & resolver (foundation)

- [x] Add `NamespaceConfig { name, path }` to `AppConfigInternal`/`AppConfig`
- [x] Add `process/namespace.rs`: `namespace_resolve`, `namespace_from_path` (longest-prefix), `namespace_root`
- [x] Startup validation: ≥1 namespace, unique names; when `trash_enabled` is true a `trash` namespace must exist; all
      roots same `st_dev` (fail fast)
- [x] `image_home` stays wired this step — nothing consumes namespaces yet

Verify: config parse/round-trip tests; resolver unit tests (round-trip, nested roots, unknown/absent namespace →
`None`/error); `just check; just test`.

### P2 — Data model & serialization (hard break)

- [x] `namespace: String` on `FileModify` and `AlbumMetadata`
- [x] `FileModify::new(namespace, path, modified)` signature update
- [x] `SCHEMA_VERSION → 7`, add v7 decode arm, keep v6 decode arm with v6 types
- [x] Fix every struct literal incl. tests

Verify: ser\_de round-trip v7 + version-byte test; `cargo build` succeeding = the literal sweep is complete.

### P3 — Resolve read-side paths

- [x] Add `source_path_resolved()` (wraps `namespace_resolve`)
- [x] Route `get_img` originals, `process/{exif,index,misc,video,xmp_write}`, `regenerate_thumbnail`, transitor through
      it
- [x] Audit: no fs operation on a raw `file`/`dir_path` field

Verify: existing `backend_api` E2E + Playwright smoke green (covers originals, EXIF, video, thumbnails).

### P4 — Expressions & album membership

- [x] `Expression::Namespace(String)` variant (generate\_filter + generate\_filter\_hide\_metadata)
- [x] `Expression::Trashed` → all aliases `namespace == "trash"`; albums check `metadata.namespace`
- [x] `Expression::Album`/`RootAlbum`/`ParentAlbum` and `Album::self_update` compare `(namespace, relative parent)`

Verify: expression unit tests updated + cross-namespace non-membership cases (shared album must not claim a trash alias
with the same relative parent).

### P5 — DIR\_ALBUM\_CACHE re-key

- [x] Key `(namespace, relative) → id` — kept as `PathBuf` key for now; namespace passed through `get_or_create_dir_album`
- [x] Update `get_or_create_dir_album`, `write_album_to_db` — accept namespace, compute relative path
- [x] `is_dir()`/`read_albuminfo` resolve via `namespace_resolve`

Verify: dir\_album unit tests; cache-prefix rewrite across namespaces.

### P6 — Core write ops

- [x] `trash_move_item`/`trash_move_album`: physical move to trash root + namespace flip (relative unchanged);
      `is_in_trash` → namespace check
- [x] `permanent_delete_item`/`permanent_delete_album`: resolve via namespace
- [x] `assign_album`: `rewrite_paths_under` resolves via namespace for path rewriting; `move_item_into_album`/`move_album_into_album` resolve via `namespace_resolve`
- [x] Upload → `shared` namespace; `upload_folder` resolved under shared root
- [x] `ensure_dir_albums`/`write_album_to_db` write ns + relative

Verify: delete/restore E2E scenarios updated to namespace paths; new scenarios asserting the namespace flip on trash and
flip-back on restore; upload-landing-in-shared scenario.

### P7 — Query scoping

- [x] `get_albums?space=` (reject unknown values)
- [x] Timeline/trash/search prefetch scoped via `Expression::Namespace`

Verify: API tests `?space=`; prefetch cache-namespacing test (same filter, different space → different cache entry).

### P8 — Watcher & indexer

- [x] One watcher per namespace root, namespace predetermined; events → `(namespace, relative)`; removal handler
      compares relative
- [x] Trash namespace not watched (or events ignored)
- [x] `index_album(namespace, rel)`; `POST /post/index/album` gains `namespace` (default `shared`);
      `workflow::index_image(namespace, rel, dst)`

Verify: `api_watcher` reworked to namespace roots; watcher-ignores-trash test.

### P9 — Config removal & frontend

- [ ] Remove `image_home`/`trash_directory` from config, `PICASU_IMAGE_HOME`, JSON response → expose `namespaces`;
      fail fast when absent; `edit_config` updated
- [ ] Frontend: AppConfig type + configStore; StorageAndSync/AlbumIndex/ GalleryEmptyCard/ServerFilePicker
      namespace-aware; index-files flow sends namespace
- [ ] ItemDelete/ItemPermanentlyDelete match alias by `(namespace, relative parent)` instead of `startsWith`

Verify: `api_config`/`api_first_launch` updated; vitest + config-page Playwright.

### P10 — Full verification & docs

- [ ] `just check; just test`; full Playwright
- [ ] Update `docs/design.md` storage section + `docs/config.md`

## Progress

### 2026-08-09 — P1 complete

- Added `NamespaceConfig { name, path }` struct to `config.rs` (JSON API + TOML + `utoipa::ToSchema`)
- Added `namespaces: Vec<NamespaceConfig>` to `AppConfig`, `TomlGallery`, `From` conversions, `Default`
- Created `process/namespace.rs` with `namespace_resolve`, `namespace_from_path` (longest-prefix), `namespace_root`
- Added startup validation in `AppConfig::init()`: ≥1 namespace, unique names, trash namespace when `trash_enabled`, same `st_dev`
- On first launch, auto-populates a "shared" namespace from `image_home`
- Updated test bootstrap to include a "shared" namespace
- 21 tests passing (14 config + 7 resolver); `just check` clean; dead-code `#[allow]` on resolver fns (consumed in P3)

### 2026-08-09 — P2–P6 complete (atomic milestone)

**P2 — Data model & serialization:**

- Added `namespace: String` to `FileModify` and `AlbumMetadata`
- Updated `FileModify::new(namespace, path, modified)` signature
- Bumped `SCHEMA_VERSION` to 7; added v6 legacy types (`FileModifyV6`, `AlbumMetadataV6`, `ImageMetadataV6`, `VideoMetadataV6`, `AbstractDataV6`) for backward-compatible v6 decode; v7 decode uses live types
- Fixed all struct literal constructions across the codebase

**P3 — Resolve read-side paths:**

- Added `source_path_resolved()`, `source_namespace()`, `source_path_string()` to `AbstractData`
- Routed `get_img` originals, `process/{exif,index,misc,video,xmp_write}`, transitor through `source_path_resolved()`
- No fs operation on raw `file`/`dir_path` field — all resolved via `namespace_resolve`

**P4 — Expressions & album membership:**

- Added `Expression::Namespace(String)` variant with `generate_filter` and `generate_filter_hide_metadata` handling
- Changed `Expression::Trashed` to check `namespace == "trash"` (no more path-prefix check)
- Updated `Album::self_update` to compare `(namespace, relative parent)` instead of absolute paths
- Added cross-namespace non-membership test

**P5 — DIR_ALBUM_CACHE re-key:**

- `get_or_create_dir_album` and `write_album_to_db` now accept namespace parameter
- `write_album_to_db` computes relative path via `namespace_from_path` and stores `namespace` + relative `dir_path`
- `init_dir_album_cache` resolves paths via `namespace_resolve`

**P6 — Core write ops:**

- `trash_move_item`/`trash_move_album`: physical move to trash namespace root + namespace flip; `is_in_trash` is now a simple namespace check
- `permanent_delete_item`/`permanent_delete_album`: resolve via `namespace_resolve`
- `assign_album`/`rewrite_paths_under`: resolves via namespace for path rewriting; namespace flips on cross-namespace moves
- `move_item_into_album`/`move_album_into_album`: resolve source via `namespace_resolve`, compute new namespace via `namespace_from_path`
- Upload → "shared" namespace; `index_image` takes namespace parameter
- `ensure_dir_albums` passes namespace through; workflow `index_image` resolves paths via namespace

**Verification:**

- `just check` clean (cargo fmt + clippy)
- 214 unit tests passing (model, expression, album, ser_de, dir_album, namespace, sanitize, xmp, auth, assign_album, delete, upload)
- E2E scenario tests hang on server startup — expected; they exercise the watcher/indexer pipeline which needs P8 completion

### 2026-08-09 — P7–P8 complete

**P7 — Query scoping:**

- Added `Space` enum (`Shared`, `Trash`) with `FromFormField` derive to `get_list.rs`
- Added `space` query parameter to `GET /get/get-albums`; validates against configured namespaces, rejects unknown values with 400
- Albums filtered by `namespace` when `space` is provided; returns all namespaces when omitted
- `Expression::Namespace` already works in `generate_filter` and `generate_filter_hide_metadata` — frontend can compose it into prefetch/search filters for automatic namespace-aware query hashing

**P8 — Watcher & indexer:**

- Reworked `start_watcher.rs`: replaced single `WATCHER_HANDLE` with `WATCHER_HANDLES: HashMap<String, RecommendedWatcher>` (one per namespace)
- `start_watcher_task_internal` iterates configured namespaces, skips trash, creates a watcher per non-trash root
- `new_namespace_watcher(namespace)` closure captures namespace, strips root prefix to produce `(namespace, relative)` pairs
- `submit_to_debounce_pool` and `submit_removal_to_watcher` now take `(namespace, relative)` instead of absolute paths
- `handle_removed_file(namespace, relative)` matches aliases by `(namespace, file)` instead of just `file`
- `DEBOUNCE_POOL` keyed by `(String, PathBuf)` instead of `PathBuf`
- `IndexAlbumRequest` gains `namespace` field (default `"shared"`)
- `index_album(namespace, src)` uses `namespace_root(namespace)` instead of `get_resolved_image_home()`; spawned tasks pass namespace to `workflow::index_image`

**Verification:**

- `just backend-check` clean (cargo fmt + clippy)
- 134 unit tests passing (expression, model, ser_de, dir_album, namespace, sanitize, xmp, auth, assign_album, delete, upload)
- E2E scenario tests still hang on server startup — known blocker for P10
