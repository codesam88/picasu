# Delete Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement filesystem-based two-stage trash for v0.1: first delete moves to `.trash/`, second delete permanently removes, untrash restores. No `is_trashed` flag — trash state derived from filesystem location.

**Architecture:** The `.trash/` directory under `IMAGE_HOME` is a normal directory album. Trash/untrash operations reuse the existing `rewrite_paths_under()` and `rewrite_dir_album_cache_prefix()` functions from `assign_album.rs`. The `DELETE /delete/delete-data` endpoint checks the filesystem location to decide between trash-move and permanent-delete. A new `PUT /put/restore-from-trash` endpoint handles untrash. `Expression::Trashed` is removed; pages use `album:.trash` path filters.

**Tech Stack:** Rust (Rocket, redb), Vue 3 (Vuetify, Pinia), YAML scenario DSL (API E2E + Playwright)

## Global Constraints

- Follow existing config pattern for new fields (`AppConfig`, `TomlGallery`, both `From` impls, `PartialUpdateConfigRequest`, `ConfigResponse`)
- Reuse `rewrite_paths_under()` and `rewrite_dir_album_cache_prefix()` from `assign_album.rs`
- `read_only_mode` guard must gate all new endpoints
- Tests follow the YAML scenario DSL per `docs/test-strategy.md`

---

## File Structure

| File                                                              | Responsibility               | Action                                                          |
| ----------------------------------------------------------------- | ---------------------------- | --------------------------------------------------------------- |
| `backend/src/model/config.rs`                                     | App configuration            | Modify — add `trash_enabled`, `trash_directory`                 |
| `backend/src/router/put/edit_config.rs`                           | Config update endpoint       | Modify — add new fields to `PartialUpdateConfigRequest`         |
| `backend/src/router/get/get_config.rs`                            | Config read endpoint         | Modify — add new fields to `ConfigResponse`                     |
| `backend/src/model/expression.rs`                                 | Filter expression evaluation | Modify — remove `Trashed` variant                               |
| `backend/src/model/object.rs`                                     | Data object schema           | Modify — remove `is_trashed` field                              |
| `backend/src/model/abstract_data.rs`                              | Data abstraction layer       | Modify — remove `set_trashed()`, add `is_in_trash()`            |
| `backend/src/router/delete.rs`                                    | Delete endpoint              | Modify — new request shape, trash-move + permanent-delete logic |
| `backend/src/router/put/restore_from_trash.rs`                    | Untrash endpoint             | Create                                                          |
| `backend/src/router/put/mod.rs`                                   | PUT route registration       | Modify — register untrash route                                 |
| `backend/src/router/start_watcher.rs`                             | File watcher                 | Modify — ignore `.trash/` events                                |
| `backend/src/tests/bootstrap.rs`                                  | Test config helpers          | Modify — handle new config fields                               |
| `frontend/src/script/lexer/lexer.ts`                              | Query parser                 | Modify — remove `trashed` token                                 |
| `frontend/src/components/Menu/MenuItem/ItemDelete.vue`            | Delete action                | Modify — resolve aliasPath, send pairs                          |
| `frontend/src/components/Menu/MenuItem/ItemPermanentlyDelete.vue` | Permanent delete action      | Modify — resolve aliasPath, send pairs, refresh                 |
| `frontend/src/components/Menu/MenuItem/ItemRestore.vue`           | Restore action               | Modify — call untrash endpoint                                  |
| `frontend/src/components/Page/*.vue`                              | Page filter strings          | Modify — update `basicString` filters                           |

---

### Task 1: Add trash config fields

**Files:**

- Modify: `backend/src/model/config.rs:40-118,163-183,212-260`
- Modify: `backend/src/router/put/edit_config.rs:15-32,45-120`
- Modify: `backend/src/router/get/get_config.rs:12-30,42-63`
- Modify: `backend/src/tests/bootstrap.rs:57-87,99-147`

**Interfaces:**

- Produces: `APP_CONFIG.read().trash_enabled: bool`, `APP_CONFIG.read().trash_directory: String`

- [ ] **Step 1: Add fields to `AppConfig` struct**

In `backend/src/model/config.rs`, add after `use_client_timestamp_info` (line 87):

```rust
    #[serde(default = "default_true")]
    pub trash_enabled: bool,
    #[serde(default = "default_trash_directory")]
    pub trash_directory: String,
```

Add default functions:

```rust
fn default_trash_directory() -> String {
    ".trash".to_string()
}
```

(`default_true` already exists from `validate_upload_content`.)

Update `Default for AppConfig` (line 98) to include:

```rust
trash_enabled: true,
trash_directory: default_trash_directory(),
```

- [ ] **Step 2: Add fields to `TomlGallery` struct**

In `backend/src/model/config.rs`, add to `TomlGallery` (after `use_client_timestamp_info`):

```rust
    #[serde(default = "default_true")]
    pub trash_enabled: bool,
    #[serde(default = "default_trash_directory")]
    pub trash_directory: String,
```

Update `TomlGallery::default()` to include both fields.

- [ ] **Step 3: Add to both `From` impls**

In `From<TomlFile> for AppConfig` (line 212), map:

```rust
trash_enabled: t.gallery.trash_enabled,
trash_directory: t.gallery.trash_directory,
```

In `From<AppConfig> for TomlFile` (line 234), map:

```rust
trash_enabled: c.trash_enabled,
trash_directory: c.trash_directory,
```

- [ ] **Step 4: Add to `PartialUpdateConfigRequest` and handler**

In `backend/src/router/put/edit_config.rs`, add to `PartialUpdateConfigRequest`:

```rust
    pub trash_enabled: Option<bool>,
    pub trash_directory: Option<String>,
```

In `update_config_handler()`, add after `use_client_timestamp_info` handling:

```rust
if let Some(trash_enabled) = body.trash_enabled {
    current_config.trash_enabled = trash_enabled;
}
if let Some(trash_directory) = body.trash_directory {
    current_config.trash_directory = trash_directory;
}
```

- [ ] **Step 5: Add to `ConfigResponse` and handler**

In `backend/src/router/get/get_config.rs`, add to `ConfigResponse`:

```rust
    pub trash_enabled: bool,
    pub trash_directory: String,
```

In `get_config_handler()`, map:

```rust
trash_enabled: config.trash_enabled,
trash_directory: config.trash_directory.clone(),
```

- [ ] **Step 6: Add to test helpers**

In `backend/src/tests/bootstrap.rs`, add the new fields to `write_config()` and `reset_backend_state()` following the existing pattern for `validate_upload_content`.

- [ ] **Step 7: Verify**

Run: `cargo check` in `backend/`
Expected: compiles without errors

Run: `cargo test` in `backend/`
Expected: existing tests pass (new fields have defaults, backward compatible)

- [ ] **Step 8: Commit**

```bash
git add backend/src/model/config.rs backend/src/router/put/edit_config.rs backend/src/router/get/get_config.rs backend/src/tests/bootstrap.rs
git commit -m "feat: add trash_enabled and trash_directory config fields"
```

---

### Task 2: Remove `Expression::Trashed` from filter system

**Files:**

- Modify: `backend/src/model/expression.rs:33,97-103,404-412,539,551-552,646-650`
- Modify: `frontend/src/script/lexer/lexer.ts:45,83,142,196-199,283-285,347-349`

**Interfaces:**

- Consumes: nothing new
- Produces: `Expression::Trashed` variant removed; `trashed:` token removed from lexer

- [ ] **Step 1: Remove `Trashed` variant from `Expression` enum**

In `backend/src/model/expression.rs`, remove line 33:

```rust
Trashed(bool),
```

- [ ] **Step 2: Remove `Trashed` evaluation in `generate_filter()`**

In `backend/src/model/expression.rs`, remove lines 97-103 (the `Expression::Trashed` match arm in `generate_filter()`).

- [ ] **Step 3: Remove `Trashed` evaluation in `generate_filter_hide_metadata()`**

Remove lines 646-650 (the `Expression::Trashed` match arm in `generate_filter_hide_metadata()`).

- [ ] **Step 4: Remove `Trashed` from tests**

Remove the `trashed_matches_flag` test (lines 404-412). Remove `Expression::Trashed` references from `And` test (line 539) and `Or` test (lines 551-552). Update these tests to use a different expression variant (e.g., `Expression::Favorite(true)`).

- [ ] **Step 5: Remove `trashed` token from lexer**

In `frontend/src/script/lexer/lexer.ts`:

- Remove line 45: `const Trashed: TokenType = createToken({ name: 'Trashed', pattern: /trashed:/ })`
- Remove from `allTokens` array (line 83)
- Remove `trashedExpression` parser rule (lines 196-199) and its reference in `atomicExpression` (line 142)
- Remove visitor dispatch (lines 283-285) and visitor implementation (lines 347-349)

- [ ] **Step 6: Verify**

Run: `cargo test` in `backend/`
Expected: all tests pass (no remaining references to `Expression::Trashed`)

Run: `cd frontend && npx tsc --noEmit`
Expected: no TypeScript errors

- [ ] **Step 7: Commit**

```bash
git add backend/src/model/expression.rs frontend/src/script/lexer/lexer.ts
git commit -m "refactor: remove Expression::Trashed from filter system"
```

---

### Task 3: Remove `is_trashed` from data model

**Files:**

- Modify: `backend/src/model/object.rs:52,68`
- Modify: `backend/src/model/abstract_data.rs:429-435`

**Interfaces:**

- Consumes: `Expression::Trashed` already removed (Task 2)
- Produces: `ObjectSchema.is_trashed` field removed; `set_trashed()` removed

- [ ] **Step 1: Remove `is_trashed` field from `ObjectSchema`**

In `backend/src/model/object.rs`, remove line 52:

```rust
pub is_trashed: bool,
```

Remove the initialization at line 68:

```rust
is_trashed: false,
```

- [ ] **Step 2: Remove `set_trashed()` from `AbstractData`**

In `backend/src/model/abstract_data.rs`, remove lines 429-435 (the `set_trashed` method).

- [ ] **Step 3: Fix compilation errors**

Search for all references to `is_trashed` and `set_trashed` in the codebase and remove/update them. Key locations:

- `backend/src/router/put/edit_flags.rs` — remove the `is_trashed` handling in `EditFlagsData` and the `set_trashed` call
- `backend/src/model/album.rs` — remove `is_trashed` checks in `self_update()` (lines 60, 72)

For `edit_flags.rs`: remove `is_trashed` from `EditFlagsData`, remove the `set_trashed` call, remove the album update trigger for trash changes.

For `album.rs`: the `!img.object.is_trashed` / `!vid.object.is_trashed` filters in `self_update()` need to be removed or replaced. Since trash items will be physically in `.trash/` and excluded by path filter, the album self-update should not filter by trash state. Remove these checks.

- [ ] **Step 4: Verify**

Run: `cargo check` in `backend/`
Expected: compiles without errors

Run: `cargo test` in `backend/`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add backend/src/model/object.rs backend/src/model/abstract_data.rs backend/src/router/put/edit_flags.rs backend/src/model/album.rs
git commit -m "refactor: remove is_trashed from data model"
```

---

### Task 4: Delete endpoint — trash-move + permanent-delete

**Files:**

- Modify: `backend/src/router/delete.rs`

**Interfaces:**

- Consumes: `APP_CONFIG.read().trash_enabled`, `APP_CONFIG.read().trash_directory`, `rewrite_paths_under()`, `rewrite_dir_album_cache_prefix()`
- Produces: `DELETE /delete/delete-data` accepts `DeleteItem { index, alias_path }`, checks `.trash/` prefix to decide action

This is the core task. The trash-move for albums reuses the same pattern as `move_album_into_album()` in `assign_album.rs`: `fs::rename` + `rewrite_paths_under()` + `rewrite_dir_album_cache_prefix()`.

- [x] **Step 1: Define new request structs**

In `backend/src/router/delete.rs`, replace `DeleteList`:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteItem {
    pub index: usize,
    pub alias_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteList {
    pub delete_list: Vec<DeleteItem>,
    pub timestamp: i64,
}
```

- [x] **Step 2: Add imports**

Add to the imports in `delete.rs`:

```rust
use crate::model::config::APP_CONFIG;
use crate::process::dir_album::{get_dir_path_for_album, get_parent_album_id, mark_album_for_update, rewrite_dir_album_cache_prefix};
use crate::router::put::assign_album::rewrite_paths_under;
use std::fs;
```

Note: `rewrite_paths_under` needs to be made `pub` in `assign_album.rs`. Add `pub` to its signature.

- [x] **Step 3: Implement `compute_trash_root()` helper**

```rust
fn compute_trash_root() -> PathBuf {
    let config = APP_CONFIG.get().expect("APP_CONFIG not initialized").read().expect("lock poisoned");
    let image_home = config.image_home.as_ref().expect("image_home not set");
    image_home.join(&config.trash_directory)
}
```

- [x] **Step 4: Implement `is_in_trash()` helper**

```rust
fn is_in_trash(alias_path: &str, trash_root: &Path) -> bool {
    Path::new(alias_path).starts_with(trash_root)
}
```

- [x] **Step 5: Implement `trash_move_item()` for images/videos**

```rust
fn trash_move_item(
    data_table: &redb::Table<...>,
    hash: &str,
    alias_path: &str,
    trash_root: &Path,
) -> Result<(), AppError> {
    let mut abstract_data: AbstractData = data_table.get(hash)...?.value();
    let alias_idx = abstract_data.alias().iter().position(|a| a.file == alias_path)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias not found"))?;

    let source = Path::new(alias_path);
    let relative = source.strip_prefix(trash_root.parent().unwrap_or(trash_root))
        .unwrap_or(source);
    let dest = trash_root.join(relative);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(source, &dest)?;

    abstract_data.alias_mut()[alias_idx].file = dest.to_string_lossy().into_owned();
    data_table.insert(hash, abstract_data)?;
    Ok(())
}
```

- [x] **Step 6: Implement `trash_move_album()` for albums**

This follows the same pattern as `move_album_into_album()` in `assign_album.rs`:

```rust
fn trash_move_album(
    data_table: &redb::Table<...>,
    hash: &str,
    alias_path: &str,
    trash_root: &Path,
) -> Result<(), AppError> {
    let abstract_data: AbstractData = data_table.get(hash)...?.value();
    let AbstractData::Album(album) = &abstract_data else {
        return Err(AppError::new(ErrorKind::InvalidInput, "Expected album"));
    };

    let source_dir = PathBuf::from(&album.metadata.dir_path);
    let dir_name = source_dir.file_name()...;
    let dest_dir = trash_root.join(dir_name);

    fs::create_dir_all(trash_root)?;
    fs::rename(&source_dir, &dest_dir)?;

    // Rewrite all records under source_dir to dest_dir
    for entry in data_table.iter() {
        let (key, value) = entry?;
        let mut data: AbstractData = value.value();
        if rewrite_paths_under(&mut data, &source_dir, &dest_dir) {
            data_table.insert(key.value(), data)?;
        }
    }

    rewrite_dir_album_cache_prefix(&source_dir, &dest_dir);

    if let Some(parent_id) = get_parent_album_id(&source_dir) {
        mark_album_for_update(parent_id);
    }

    Ok(())
}
```

- [x] **Step 7: Implement `permanent_delete_item()` for images/videos**

```rust
fn permanent_delete_item(
    data_table: &redb::Table<...>,
    hash: &str,
    alias_path: &str,
) -> Result<Option<AbstractData>, AppError> {
    let mut abstract_data: AbstractData = data_table.get(hash)...?.value();
    let alias_idx = abstract_data.alias().iter().position(|a| a.file == alias_path)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias not found"))?;

    let file_path = Path::new(alias_path);
    let _ = fs::remove_file(file_path);
    let sidecar = file_path.with_extension("xmp");
    let _ = fs::remove_file(&sidecar);

    abstract_data.alias_mut().remove(alias_idx);

    if abstract_data.alias().is_empty() {
        // Last alias — delete thumbnail and remove record
        let thumb = abstract_data.compressed_path();
        if !thumb.is_empty() {
            let _ = fs::remove_file(&thumb);
        }
        data_table.remove(hash)?;
        Ok(Some(abstract_data))
    } else {
        // Aliases remain — flush updated record
        data_table.insert(hash, abstract_data)?;
        Ok(None)
    }
}
```

- [x] **Step 8: Implement `permanent_delete_album()` with cascade**

```rust
fn permanent_delete_album(
    data_table: &redb::Table<...>,
    hash: &str,
    alias_path: &str,
) -> Result<Vec<AbstractData>, AppError> {
    let abstract_data: AbstractData = data_table.get(hash)...?.value();
    let AbstractData::Album(album) = &abstract_data else {
        return Err(AppError::new(ErrorKind::InvalidInput, "Expected album"));
    };

    let dir_path = PathBuf::from(&album.metadata.dir_path);
    let mut removed = Vec::new();

    // Collect all records under this album's directory
    let mut to_remove = Vec::new();
    for entry in data_table.iter() {
        let (key, value) = entry?;
        let data: AbstractData = value.value();
        let dominated = match &data {
            AbstractData::Album(a) => PathBuf::from(&a.metadata.dir_path).starts_with(&dir_path),
            AbstractData::Image(_) | AbstractData::Video(_) => {
                data.alias().iter().any(|a| Path::new(&a.file).starts_with(&dir_path))
            }
        };
        if dominated {
            to_remove.push((key.value().to_string(), data));
        }
    }

    // Delete files and records
    for (key, data) in to_remove {
        match &data {
            AbstractData::Album(_) => {
                // Child album — will be handled by recursive call or dir removal
            }
            AbstractData::Image(_) | AbstractData::Video(_) => {
                for alias in data.alias() {
                    let _ = fs::remove_file(&alias.file);
                    let _ = fs::remove_file(Path::new(&alias.file).with_extension("xmp"));
                }
                let thumb = data.compressed_path();
                if !thumb.is_empty() {
                    let _ = fs::remove_file(&thumb);
                }
            }
        }
        data_table.remove(&*key)?;
        removed.push(data);
    }

    // Remove the album record itself
    data_table.remove(hash)?;
    removed.push(abstract_data);

    // Remove directory from disk
    let _ = fs::remove_dir_all(&dir_path);

    // Evict from cache
    // (use rewrite to empty or direct cache access)

    Ok(removed)
}
```

- [x] **Step 9: Refactor `process_deletes()` to dispatch based on `.trash/` prefix**

Replace the current `process_deletes` with logic that:

1. Reads `trash_enabled` and `trash_directory` from `APP_CONFIG`
2. Computes `trash_root`
3. For each `DeleteItem`:
   - Resolves `index` → `AbstractData`
   - Checks `is_in_trash(alias_path, trash_root)`
   - If not in trash and trash_enabled: calls `trash_move_album` or `trash_move_item`
   - If in trash or !trash_enabled: calls `permanent_delete_album` or `permanent_delete_item`
4. Collects affected album IDs for `AlbumSelfUpdateTask`

- [x] **Step 10: Update `delete_data()` handler**

Update the handler to use the new `DeleteItem` struct. The handler signature stays the same. Update the `process_deletes` call to pass the new structure.

- [x] **Step 11: Make `rewrite_paths_under` public**

In `backend/src/router/put/assign_album.rs`, change `fn rewrite_paths_under` to `pub fn rewrite_paths_under`. Add it to the module's public API.

- [x] **Step 12: Verify**

Run: `cargo check` in `backend/`
Expected: compiles without errors

- [x] **Step 13: Commit**

```bash
git add backend/src/router/delete.rs backend/src/router/put/assign_album.rs
git commit -m "feat: delete endpoint with filesystem-based trash and permanent delete"
```

---

### Task 5: Untrash endpoint

**Files:**

- Create: `backend/src/router/put/restore_from_trash.rs`
- Modify: `backend/src/router/put/mod.rs`

**Interfaces:**

- Consumes: `APP_CONFIG`, `rewrite_paths_under()`, `rewrite_dir_album_cache_prefix()`
- Produces: `PUT /put/restore-from-trash` endpoint

- [ ] **Step 1: Create `restore_from_trash.rs`**

Create `backend/src/router/put/restore_from_trash.rs`:

```rust
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::model::config::APP_CONFIG;
use crate::process::dir_album::{get_parent_album_id, mark_album_for_update, rewrite_dir_album_cache_prefix};
use crate::process::transitor::index_to_abstract_data;
use crate::router::auth::{GuardAuth, GuardReadOnlyMode};
use crate::router::put::assign_album::rewrite_paths_under;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{open_data_table, open_tree_snapshot_table};
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::{BATCH_COORDINATOR, INDEX_COORDINATOR};
use futures::future::try_join_all;
use log::warn;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RestoreItem {
    pub index: usize,
    pub alias_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RestoreList {
    pub restore_list: Vec<RestoreItem>,
    pub timestamp: i64,
}
```

The handler follows the same `index_to_abstract_data` → find alias → operate pattern as `delete.rs`. Core logic:

```rust
for item in restore_list {
    let abstract_data = index_to_abstract_data(&tree_snapshot, &data_table, item.index)?;
    let alias_idx = abstract_data.alias().iter()
        .position(|a| a.file == item.alias_path)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias not found"))?;

    if !is_in_trash(&item.alias_path, &trash_root) {
        return Err(AppError::new(ErrorKind::InvalidInput, "Item is not in trash"));
    }

    let source = Path::new(&item.alias_path);
    let relative = source.strip_prefix(&trash_root).unwrap_or(source);
    let dest = image_home.join(relative);

    match &abstract_data {
        AbstractData::Album(_) => {
            // Album restore: fs::rename + rewrite_paths_under + cache rewrite
            fs::rename(source, &dest)?;
            // Rewrite all records under source → dest (same as trash_move_album)
            // rewrite_dir_album_cache_prefix(&source_dir, &dest_dir);
        }
        AbstractData::Image(_) | AbstractData::Video(_) => {
            // Item restore: fs::rename + update single alias
            fs::rename(source, &dest)?;
            abstract_data.alias_mut()[alias_idx].file = dest.to_string_lossy().into_owned();
            data_table.insert(&*hash, abstract_data)?;
        }
    }
}
```

- [ ] **Step 2: Register route**

In `backend/src/router/put/mod.rs`, add:

```rust
pub mod restore_from_trash;
```

Add the route to the `generate_put_routes()` function:

```rust
routes![..., restore_from_trash::restore_from_trash]
```

- [ ] **Step 3: Verify**

Run: `cargo check`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add backend/src/router/put/restore_from_trash.rs backend/src/router/put/mod.rs
git commit -m "feat: add PUT /put/restore-from-trash endpoint"
```

---

### Task 6: Frontend — delete + restore wiring

**Files:**

- Modify: `frontend/src/components/Menu/MenuItem/ItemDelete.vue`
- Modify: `frontend/src/components/Menu/MenuItem/ItemPermanentlyDelete.vue`
- Modify: `frontend/src/components/Menu/MenuItem/ItemRestore.vue`
- Modify: `frontend/src/components/Page/TrashedPage.vue`
- Modify: `frontend/src/components/Page/TimelinePage.vue`
- Modify: `frontend/src/components/Page/FavoritePage.vue`
- Modify: `frontend/src/components/Page/VideosPage.vue`
- Modify: `frontend/src/components/Page/ArchivedPage.vue`
- Modify: `frontend/src/components/Page/AlbumsPage.vue`
- Modify: `frontend/src/components/Page/AlbumContentsPage.vue`

**Interfaces:**

- Consumes: `DELETE /delete/delete-data` with new `DeleteItem` shape, `PUT /put/restore-from-trash`
- Produces: trash/restore UI wired to backend

- [ ] **Step 1: Add `resolveAliasPath` helper**

Create or update `frontend/src/script/utils/resolveAliasPath.ts`:

```typescript
import type { EnrichedUnifiedData } from "../schemas";

export function resolveAliasPath(
  item: EnrichedUnifiedData,
  albumDir: string | null,
): string {
  if (albumDir) {
    const match = item.alias.find((a) => a.file.startsWith(albumDir));
    if (match) return match.file;
  }
  return item.alias[0].file;
}
```

- [ ] **Step 2: Update `ItemDelete.vue`**

Import `resolveAliasPath`. Before calling the delete API, resolve the alias path for each item. Build the `deleteList` array with `{ index, aliasPath }` pairs. Send as the request body.

- [ ] **Step 3: Update `ItemPermanentlyDelete.vue`**

Same as Step 2. Additionally, call `refreshGalleryAfterMutation()` after successful delete to sync the view.

- [ ] **Step 4: Update `ItemRestore.vue`**

Import `resolveAliasPath`. Build `restoreList` with `{ index, aliasPath }` pairs. Call `PUT /put/restore-from-trash`. Call `refreshGalleryAfterMutation()` on success.

- [ ] **Step 5: Update page filter strings**

In each page component, replace `trashed:false` with `not(album:.trash)` in the `basicString`:

| File                    | Before          | After               |
| ----------------------- | --------------- | ------------------- |
| `TimelinePage.vue`      | `trashed:false` | `not(album:.trash)` |
| `FavoritePage.vue`      | `trashed:false` | `not(album:.trash)` |
| `VideosPage.vue`        | `trashed:false` | `not(album:.trash)` |
| `ArchivedPage.vue`      | `trashed:false` | `not(album:.trash)` |
| `AlbumsPage.vue`        | `trashed:false` | `not(album:.trash)` |
| `AlbumContentsPage.vue` | `trashed:false` | `not(album:.trash)` |
| `TrashedPage.vue`       | `trashed:true`  | `album:.trash`      |

- [ ] **Step 6: Verify**

Run: `cd frontend && npx tsc --noEmit`
Expected: no TypeScript errors

Run: `cd frontend && npm run lint`
Expected: no lint errors

- [ ] **Step 7: Commit**

```bash
git add frontend/src/components/Menu/MenuItem/ItemDelete.vue frontend/src/components/Menu/MenuItem/ItemPermanentlyDelete.vue frontend/src/components/Menu/MenuItem/ItemRestore.vue frontend/src/components/Page/ frontend/src/script/utils/resolveAliasPath.ts
git commit -m "feat: wire frontend delete/restore to filesystem-based trash"
```

---

### Task 7: Watcher — ignore `.trash/` events

**Files:**

- Modify: `backend/src/router/start_watcher.rs`

**Interfaces:**

- Consumes: `APP_CONFIG.read().trash_directory`
- Produces: watcher ignores file events under `.trash/` prefix

- [ ] **Step 1: Add trash path check**

In the watcher event handler, early-return if the event path starts with the trash directory:

```rust
let config = APP_CONFIG.get().expect("APP_CONFIG not initialized").read().expect("lock poisoned");
let trash_root = config.image_home.as_ref().unwrap().join(&config.trash_directory);
drop(config);

// In the event handler loop:
if event_path.starts_with(&trash_root) {
    continue;
}
```

- [ ] **Step 2: Verify**

Run: `cargo check`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add backend/src/router/start_watcher.rs
git commit -m "feat: watcher ignores .trash/ directory events"
```

---

### Task 8: API E2E test scenarios

**Files:**

- Create: `backend/tests/scenarios/trash_and_permanent_delete_image.yaml`
- Create: `backend/tests/scenarios/trash_and_permanent_delete_album.yaml`
- Create: `backend/tests/scenarios/trash_multi_alias.yaml`
- Create: `backend/tests/scenarios/permanent_delete_alias_preserves_record.yaml`
- Create: `backend/tests/scenarios/trash_disabled_permanent_delete.yaml`
- Create: `backend/tests/scenarios/custom_trash_directory.yaml`
- Create: `backend/tests/scenarios/trash_read_only_mode_blocked.yaml`
- Create: `backend/tests/scenarios/trash_edge_cases.yaml`
- Create: `backend/tests/scenarios/untrash_image_and_album.yaml`
- Create: `backend/tests/scenarios/untrash_conflict_handling.yaml`
- Create: `backend/tests/scenarios/untrash_not_in_trash_rejected.yaml`
- Create: `backend/tests/scenarios/watcher_ignores_trash_events.yaml`

**Interfaces:**

- Consumes: all backend endpoints implemented in Tasks 1-7
- Produces: validated behavior

- [ ] **Step 1: Update existing delete scenario**

Update `backend/tests/scenarios/delete_removes_file_and_sidecar_z3.yaml` to use the new `DeleteItem` request shape with `aliasPath`.

- [ ] **Step 2: Create `trash_and_permanent_delete_image.yaml`**

```yaml
name: trash then permanent delete removes image
given:
  - photo: /pool/img.jpg
    id_as: $photo
when:
  - call: POST /get/prefetch?locate=${photo}
    capture:
      ts: response.prefetch.timestamp
      idx: response.prefetch.locateTo
  # First delete: moves to trash
  - call: DELETE /delete/delete-data
    raw_body: '{"deleteList":[{"index":${idx},"aliasPath":"${data_path}/pool/img.jpg"}],"timestamp":${ts}}'
    then:
      - response.status: 200
      - file_absent: /pool/img.jpg
      - file_exists: /.trash/pool/img.jpg
  # Re-capture after tree update
  - call: POST /get/prefetch?locate=${photo}
    capture:
      ts2: response.prefetch.timestamp
      idx2: response.prefetch.locateTo
  # Second delete: permanent
  - call: DELETE /delete/delete-data
    raw_body: '{"deleteList":[{"index":${idx2},"aliasPath":"${data_path}/.trash/pool/img.jpg"}],"timestamp":${ts2}}'
    then:
      - response.status: 200
      - file_absent: /.trash/pool/img.jpg
```

Note: `${data_path}` is automatically resolved by the scenario interpreter to `IMAGE_HOME`. Use it for all `aliasPath` values.

- [ ] **Step 3: Create remaining API scenarios**

Follow the same pattern for each scenario. Key scenarios:

**`trash_and_permanent_delete_album.yaml`:** Create dir album with photos, trash album, verify all moved to `.trash/`, permanent delete, verify all removed.

**`trash_multi_alias.yaml`:** Create photo, copy to second location (creating alias), trash one alias, verify other still exists.

**`permanent_delete_alias_preserves_record.yaml`:** Photo with two aliases, one in `.trash/`, one live. Permanent delete the trash alias. Verify the live alias and record survive.

**`trash_disabled_permanent_delete.yaml`:** `given config trash_enabled: false`, delete photo, verify immediate removal (no `.trash/` move).

**`trash_edge_cases.yaml`:** Delete with nonexistent alias_path (warn+skip), delete already-deleted item (idempotent), verify cache eviction after album trash.

**`untrash_image_and_album.yaml`:** Trash image, trash album, untrash both, verify files restored to original locations.

**`untrash_conflict_handling.yaml`:** Trash photo, create new file at original location, untrash with `on_conflict: skip` (stays in trash), untrash with `on_conflict: rename` (restored as `_1`).

**`untrash_not_in_trash_rejected.yaml`:** Attempt untrash on a non-trashed photo → 400.

- [ ] **Step 4: Run all scenarios**

Run: `cargo test` in `backend/`
Expected: all new scenarios pass

- [ ] **Step 5: Commit**

```bash
git add backend/tests/scenarios/
git commit -m "test: add delete lifecycle API E2E scenarios"
```

---

### Task 9: Unit tests for trash helpers

**Files:**

- Modify: `backend/src/router/delete.rs` (add `#[cfg(test)]` module)

**Interfaces:**

- Consumes: `compute_trash_root()`, `is_in_trash()`, trash-move logic
- Produces: validated helper behavior

- [ ] **Step 1: Add unit tests for `is_in_trash()`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn is_in_trash_detects_trash_path() {
        let trash_root = Path::new("/home/user/images/.trash");
        assert!(is_in_trash("/home/user/images/.trash/album/img.jpg", trash_root));
    }

    #[test]
    fn is_in_trash_rejects_normal_path() {
        let trash_root = Path::new("/home/user/images/.trash");
        assert!(!is_in_trash("/home/user/images/album/img.jpg", trash_root));
    }

    #[test]
    fn is_in_trash_rejects_partial_prefix() {
        let trash_root = Path::new("/home/user/images/.trash");
        assert!(!is_in_trash("/home/user/images/.trashy/album/img.jpg", trash_root));
    }
}
```

- [ ] **Step 2: Run unit tests**

Run: `cargo test is_in_trash`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add backend/src/router/delete.rs
git commit -m "test: add unit tests for trash path helpers"
```

---

### Task 10: Playwright UI test scenarios

**Files:**

- Create: `frontend/tests/playwright/scenarios/delete-to-trash-flow.yaml`
- Create: `frontend/tests/playwright/scenarios/trash-permanently-delete.yaml`
- Create: `frontend/tests/playwright/scenarios/trash-restore.yaml`
- Create: `frontend/tests/playwright/scenarios/trash-batch-operations.yaml`
- Create: `frontend/tests/playwright/scenarios/trash-multi-alias-visibility.yaml`
- Create: `frontend/tests/playwright/scenarios/trash-empty-state.yaml`

**Interfaces:**

- Consumes: frontend UI wired in Task 6
- Produces: validated UI behavior

- [ ] **Step 1: Update existing `delete-photo-permanently.yaml`**

Update `frontend/tests/playwright/scenarios/delete-photo-permanently.yaml` to reflect the new two-stage flow (delete → trash → permanent delete from trash page).

- [ ] **Step 2: Create `delete-to-trash-flow.yaml`**

```yaml
name: Delete photo and album moves both to trash
covers:
  api:
    - DELETE /delete/delete-data
  ui:
    - Delete menu moves item to trash
    - Trash page shows trashed items
given:
  - dir_album: trash_test/pool
  - photo: trash_test/pool/img01.jpg
steps:
  - when:
      - navigate: /albums
    assert:
      - ui.visible: main/
  - when:
      - click.text: trash_test
    assert:
      - ui.visible: main/
  # Navigate to album, delete photo
  - when:
      - click.text: pool
    assert:
      - ui.visible: main/
  - when:
      - click.first: true
    assert:
      - ui.visible: main/
  - when:
      - click.icon: mdi-information-outline
    assert:
      - ui.visible: main/
  - when:
      - click.testid: photo-menu
    assert:
      - ui.visible: main/
  - when:
      - click: option/Delete
    assert:
      - ui.visible: main/
  # Verify in trash
  - when:
      - navigate: /trashed
    assert:
      - ui.visible: main/
      - ui.text_visible: img01
```

- [ ] **Step 3: Create remaining Playwright scenarios**

Follow the same pattern. Each scenario seeds state, navigates, performs actions, and asserts.

- [ ] **Step 4: Run Playwright tests**

Run: `cd frontend && npx playwright test --grep "UI scenarios"`
Expected: all scenarios pass

- [ ] **Step 5: Commit**

```bash
git add frontend/tests/playwright/scenarios/
git commit -m "test: add delete lifecycle Playwright UI scenarios"
```

---

### Task 11: Final verification

- [ ] **Step 1: Run full test suite**

Run: `just check` (lint)
Run: `just test` (backend cargo test + frontend vitest)
Expected: all green

- [ ] **Step 2: Run Playwright**

Run: `just frontend-playwright`
Expected: all scenarios pass

- [ ] **Step 3: Update plan status**

Set `delete-from-disk.md` status to `done` if all tasks complete.

- [ ] **Step 4: Final commit (if needed)**

```bash
git add -A
git commit -m "feat: complete filesystem-based delete lifecycle for v0.1"
```
