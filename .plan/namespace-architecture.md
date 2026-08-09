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

- [ ] `namespace: String` on `FileModify` and `AlbumMetadata`
- [ ] `FileModify::new(path, namespace, modified)` signature update
- [ ] `SCHEMA_VERSION → 7`, add v7 decode arm, drop the v6 arm
- [ ] Fix every struct literal incl. tests

Verify: ser\_de round-trip v7 + version-byte test; `cargo build` succeeding = the literal sweep is complete.

### P3 — Resolve read-side paths

- [ ] Add `source_path_resolved()` (wraps `namespace_resolve`)
- [ ] Route `get_img` originals, `process/{exif,index,misc,video,xmp_write}`, `regenerate_thumbnail`, transitor through
      it
- [ ] Audit: no fs operation on a raw `file`/`dir_path` field

Verify: existing `backend_api` E2E + Playwright smoke green (covers originals, EXIF, video, thumbnails).

### P4 — Expressions & album membership

- [ ] `Expression::Namespace(String)` variant (generate\_filter + generate\_filter\_hide\_metadata)
- [ ] `Expression::Trashed` → all aliases `namespace == "trash"`; albums check `metadata.namespace`
- [ ] `Expression::Album`/`RootAlbum`/`ParentAlbum` and `Album::self_update` compare `(namespace, relative parent)`

Verify: expression unit tests updated + cross-namespace non-membership cases (shared album must not claim a trash alias
with the same relative parent).

### P5 — DIR\_ALBUM\_CACHE re-key

- [ ] Key `(namespace, relative) → id`
- [ ] Update `get_or_create_dir_album`, `get_parent_album_id`, `get_dir_path_for_album`, `get_album_id_for_dir`,
      `mark_dir_albums_for_path`, `rewrite_dir_album_cache_prefix`, `remove_dir_album_from_cache`, `init_dir_album_cache`
- [ ] `is_dir()`/`read_albuminfo` resolve via `namespace_resolve`

Verify: dir\_album unit tests; cache-prefix rewrite across namespaces.

### P6 — Core write ops

- [ ] `trash_move_item`/`trash_move_album`: physical move to trash root + namespace flip (relative unchanged);
      `is_in_trash` → namespace check
- [ ] `permanent_delete_item`/`permanent_delete_album`: resolve via namespace
- [ ] `assign_album`: split `rewrite_paths_under` into `rewrite_relative_paths_under(old_rel, new_rel)` (in-namespace
      moves) and a namespace-flip for restore; `move_item_into_album`/`move_album_into_album` resolve via
      `source_path_resolved()`
- [ ] Upload → `shared` namespace; `upload_folder` resolved under shared root
- [ ] `ensure_dir_albums`/`write_album_to_db` write ns + relative

Verify: delete/restore E2E scenarios updated to namespace paths; new scenarios asserting the namespace flip on trash and
flip-back on restore; upload-landing-in-shared scenario.

### P7 — Query scoping

- [ ] `get_albums?space=` (reject unknown values)
- [ ] Timeline/trash/search prefetch scoped via `Expression::Namespace`

Verify: API tests `?space=`; prefetch cache-namespacing test (same filter, different space → different cache entry).

### P8 — Watcher & indexer

- [ ] One watcher per namespace root, namespace predetermined; events → `(namespace, relative)`; removal handler
      compares relative
- [ ] Trash namespace not watched (or events ignored)
- [ ] `index_album(namespace, rel)`; `POST /post/index/album` gains `namespace` (default `shared`);
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
