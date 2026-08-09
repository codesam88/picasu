---
status: open
type: feature
priority: high
area: backend
---

## Context

Replace the `IMAGE_HOME`-centric model with namespace-aware storage. Each
namespace (shared, trash, future user-specific) has its own independent
filesystem root configured in the backend. DB records store namespace +
relative path; the backend resolves full paths at runtime.

`IMAGE_HOME` becomes obsolete. Thumbnails remain in `DATA_HOME` (transient
object store, not under any namespace).

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

Replace `image_home`, `shared_directory`, `trash_directory` with a namespace
registry:

```toml
[[namespace]]
name = "shared"
path = "/mnt/photos/shared"

[[namespace]]
name = "trash"
path = "/mnt/photos/trash"
```

`data_home` stays for thumbnails and internal DB.

### Path resolver (`process/namespace.rs`)

- `namespace_resolve(namespace, relative) -> PathBuf` — full path from config
- `namespace_from_path(absolute) -> Option<(String, String)>` — reverse:
  absolute to (namespace, relative). Returns `None` if not under any namespace.
- `namespace_root(namespace) -> PathBuf` — root dir for a namespace

All three read from the namespace registry in `APP_CONFIG`.

### Expression system

- `Expression::Trashed` — check `namespace == "trash"` (no more path-prefix check)
- `Expression::Album(album_id)` — resolve album namespace + path, match aliases
- `Expression::Path(...)` — match namespace-relative path

### Watcher

- One watcher per namespace root, configured at startup.
- Each watcher's namespace is predetermined from its startup config (the
  namespace name and root path it was created for).
- On file event: strip the root path prefix, return `(namespace, relative_path)`
  to the indexer. No runtime namespace identification needed.
- Skip "trash" namespace events (trash watcher is not created, or its events
  are ignored at dispatch time).

### Filesystem constraint

All namespace roots must be on the same filesystem (so `fs::rename` works
across namespaces). Assert this on startup by comparing `stat().st_dev` across
all configured namespace paths. Fail fast if any differ.

### Indexer

- `index_album(namespace: &str, relative_src: &str)`
- API: `POST /post/index/album` gains `namespace` field (default "shared")
- `ensure_dir_albums` creates albums with correct namespace

### Upload

- Default target: namespace "shared", subdirectory from `upload_folder`
- Upload to album: album's namespace + dir_path already recorded

### Trash/restore

- Physical move: `source_root + relative` to `trash_root + relative`
- Update alias: namespace changes, file stays same relative path

### Queries

- `get_albums?space=shared` — filter `namespace == "shared"`
- Timeline: `namespace == "shared"`, Trash: `namespace == "trash"`
- Album contents: match aliases by namespace + parent directory

### `DIR_ALBUM_CACHE`

- `HashMap<(String, String), ArrayString<64>>` — (namespace, relative_path) to album ID

### Serialization

Bump schema version. Add `namespace` to `FileModify` and `AlbumMetadata`.
Migration: parse absolute paths to (namespace, relative) using old config.

## Tasks

### Phase 1: Data model

- [ ] Add `namespace: String` to `FileModify`
- [ ] Add `namespace: String` to `AlbumMetadata`
- [ ] Bump serialization schema version
- [ ] Migration: parse absolute paths to (namespace, relative)

### Phase 2: Config

- [ ] Replace `image_home` + `shared_directory` + `trash_directory` with `Vec<NamespaceConfig>`
- [ ] `NamespaceConfig { name: String, path: String }`
- [ ] Update `ConfigResponse` to expose namespace registry
- [ ] Remove `imagePath` from config response

### Phase 3: Namespace resolver

- [ ] Create `process/namespace.rs` with `namespace_resolve`, `namespace_from_path`, `namespace_root`
- [ ] Startup assertion: all namespace roots on same filesystem (`stat().st_dev`)
- [ ] Update all callers that construct paths from `image_home`

### Phase 4: Expression system

- [ ] `Expression::Trashed` — check namespace field
- [ ] `Expression::Album` — resolve album namespace + path
- [ ] `Expression::Path` — match namespace-relative path

### Phase 5: Core operations

- [ ] `ensure_dir_albums` — pass namespace, create albums with namespace
- [ ] `get_or_create_dir_album` — namespace-aware cache key
- [ ] `trash_move_item/album` — update alias/album namespace
- [ ] `purge_empty_albums` — namespace-aware
- [ ] `assign_album` — namespace-aware
- [ ] Upload — default to "shared" namespace
- [ ] `Album::self_update` — match aliases by namespace + relative parent

### Phase 6: Watcher

- [ ] One watcher per namespace root, each configured with its namespace name
- [ ] Strip root prefix on events, return (namespace, relative_path) to indexer
- [ ] Skip "trash" namespace events
- [ ] Startup: assert all namespace roots on same filesystem

### Phase 7: Indexer

- [ ] `index_album` gains `namespace` parameter
- [ ] API: `POST /post/index/album` gains `namespace` field
- [ ] `ensure_dir_albums` uses namespace from context

### Phase 8: Frontend

- [ ] Update `AppConfig` type to include namespace registry
- [ ] Remove `imagePath` from config type
- [ ] Pages pass namespace to `fetchAlbums`
- [ ] Config page shows namespace registry

### Phase 9: Tests

- [ ] Update test bootstrap: configure namespace registry
- [ ] Update scenario helper: paths are namespace-relative
- [ ] Update all scenario assertions
- [ ] Migration tests

## Migration path

For existing installations:

1. Read old config: `image_home`, `shared_directory`, `trash_directory`
2. Construct namespace registry from old config values
3. Scan all DB records, parse absolute paths using old config
4. Write new records with (namespace, relative_path)
5. Bump schema version
