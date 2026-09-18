use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;

use crate::process::dir_album::{
    evict_dir_album, get_dir_path_for_album, get_parent_album_id, mark_album_for_update,
    rewrite_dir_album_cache_prefix,
};
use crate::process::sanitize::find_unique_path;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::INDEX_COORDINATOR;
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use arrayvec::ArrayString;
use log::warn;
use redb::{ReadableDatabase, ReadableTable};
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, utoipa::ToSchema, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum OnConflict {
    Rename,
    Merge,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignAlbumData {
    #[schema(value_type = String)]
    pub hash: ArrayString<64>,
    #[schema(value_type = String)]
    pub album_id: ArrayString<64>,
    /// Selected alias path for item records; must be absent (null) for albums.
    pub alias: Option<String>,
    pub on_conflict: OnConflict,
}

/// Outcome of an `assign_album` call, reported to the caller so the UI is never
/// silent about what happened to the selected item.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignResult {
    pub outcome: AssignOutcome,
}

/// The concrete result of a successful assign: `moved`, `renamedFrom` (an
/// auto-`-001` suffix collision), or `deduplicatedRemoved` (a merge dedup
/// pruned the redundant source copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AssignOutcome {
    Moved,
    RenamedFrom,
    DeduplicatedRemoved,
}

/// Move a media item into the album's directory on disk, update the DB alias,
/// and record the explicit album membership.  Returns 400 if the file is not
/// found at the recorded alias path (stale alias — user must re-index first).
#[utoipa::path(
        put,
        path = "/put/assign_album",
        request_body = AssignAlbumData,
        responses(
            (status = 200, description = "Item assigned to album", body = AssignResult),
            (status = 400, description = "Invalid input or item not found"),
        )
    )
]
#[put("/put/assign_album", format = "json", data = "<json_data>")]
pub async fn assign_album(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<AssignAlbumData>,
) -> AppResult<Json<AssignResult>> {
    let _ = auth?;
    let _ = read_only_mode?;

    let data = json_data.into_inner();
    let hash = data.hash;
    let album_id = data.album_id;
    let on_conflict = data.on_conflict;
    let selected_alias = data.alias;

    // Resolve album's directory from the in-memory cache.
    let album_dir = get_dir_path_for_album(album_id)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found in dir cache"))?;

    if !album_dir.is_dir() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!(
                "Album directory no longer exists on disk: {} — re-index to refresh",
                album_dir.display()
            ),
        ));
    }

    let outcome = tokio::task::spawn_blocking(move || {
        move_hash_into_album(hash, album_id, &album_dir, on_conflict, selected_alias)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    // Bump the version counter so subsequent prefetch calls create a new
    // query cache entry instead of returning stale data from the snapshot
    // taken before the mutation.  (UpdateExpireTask, which normally
    // advances this counter, runs asynchronously — too late for the next
    // frontend request.)
    VERSION_COUNT_TIMESTAMP.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    INDEX_COORDINATOR
        .execute_waiting(AlbumSelfUpdateTask::new(album_id))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update album stats"))?
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("Album update failed: {e}")))?;

    Ok(Json(AssignResult { outcome }))
}

/// Dispatch on whichever kind of item `hash` resolves to: images/videos move
/// as a single file (`move_item_into_album`); albums (sub-albums) move as a
/// whole directory tree (`move_album_into_album`), since an album's `.alias()`
/// is always empty and the single-file path would reject it outright.
fn move_hash_into_album(
    hash: ArrayString<64>,
    album_id: ArrayString<64>,
    album_dir: &Path,
    on_conflict: OnConflict,
    selected_alias: Option<String>,
) -> Result<AssignOutcome, AppError> {
    let is_album = {
        let txn = TREE
            .in_disk
            .begin_read()
            .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
        let data_table = txn
            .open_table(DATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;
        let abstract_data: AbstractData = data_table
            .get(&*hash)
            .or_raise(|| (ErrorKind::Database, "Failed to look up item"))?
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Item not found in database"))?
            .value();
        matches!(abstract_data, AbstractData::Album(_))
    };

    if is_album {
        move_album_into_album(
            hash,
            album_id,
            album_dir,
            on_conflict,
            selected_alias.as_deref(),
        )
    } else {
        move_item_into_album(hash, album_id, album_dir, on_conflict, selected_alias)
    }
}

/// Move a sub-album's whole directory into `target_dir` (another album's
/// directory), then update every DB record whose path lived under the old
/// directory.
///
/// When the descendant directory name does not collide with an existing target
/// path, the whole tree is renamed to `target_dir/<name>/...` under both
/// `Rename` and `Merge`, and every record under the old prefix is rewritten
/// (nested sub-albums' `dir_path` and every image/video alias). The physical
/// rename carries each file's `.xmp` sidecar along, so only the stored path
/// *strings* need updating.
///
/// On a name collision (`base_dest` already exists) the modes diverge:
/// - `Rename` keeps today's behavior: the whole directory is renamed to a
///   unique `-001` sibling (`find_unique_path`) with the same path-rewrite,
///   reported as `RenamedFrom`.
/// - `Merge` dissolves the source tree instead (reported as `Moved`): files are
///   migrated leaf-to-root into the target album root — deduping
///   byte-identical copies, auto-renaming distinct-content collisions — then
///   the emptied source directories and their dir-album records are removed.
///   Any source directory that still holds a file (and every ancestor) is kept.
fn move_album_into_album(
    album_hash: ArrayString<64>,
    target_album_id: ArrayString<64>,
    target_dir: &Path,
    on_conflict: OnConflict,
    selected_alias: Option<&str>,
) -> Result<AssignOutcome, AppError> {
    if selected_alias.is_some() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "aliases do not apply to album records",
        ));
    }
    let (old_dir, new_dir_opt, outcome, removed_dirs) = {
        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin write transaction"))?;
        let result = {
            let mut data_table = txn
                .open_table(DATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let abstract_data: AbstractData = data_table
                .get(&*album_hash)
                .or_raise(|| (ErrorKind::Database, "Failed to look up album"))?
                .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found"))?
                .value();
            let AbstractData::Album(moved_album) = abstract_data else {
                return Err(AppError::new(ErrorKind::InvalidInput, "Expected an album"));
            };

            let source_dir = PathBuf::from(&moved_album.metadata.dir_path);
            if !source_dir.is_dir() {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Album directory no longer exists on disk: {} — re-index to refresh",
                        source_dir.display()
                    ),
                ));
            }

            if target_dir == source_dir || target_dir.starts_with(&source_dir) {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Cannot move an album into itself or one of its own sub-albums",
                ));
            }

            let dir_name = source_dir.file_name().ok_or_else(|| {
                AppError::new(ErrorKind::InvalidInput, "Album directory has no name")
            })?;
            let base_dest = target_dir.join(dir_name);

            if base_dest.exists() {
                match on_conflict {
                    // Collision + Rename: whole-dir rename to a unique sibling.
                    OnConflict::Rename => {
                        let dest_dir = find_unique_path(&base_dest)?;
                        rename_whole_dir(&mut data_table, &source_dir, &dest_dir)?;
                        (
                            source_dir,
                            Some(dest_dir),
                            AssignOutcome::RenamedFrom,
                            Vec::new(),
                        )
                    }
                    // Collision + Merge: recursive leaf-to-root migration.
                    OnConflict::Merge => {
                        let removed = merge_album_tree(&mut data_table, &source_dir, target_dir)?;
                        (source_dir, None, AssignOutcome::Moved, removed)
                    }
                }
            } else {
                // No collision: both modes land the whole tree in place (Moved).
                let dest_dir = base_dest;
                rename_whole_dir(&mut data_table, &source_dir, &dest_dir)?;
                (source_dir, Some(dest_dir), AssignOutcome::Moved, Vec::new())
            }
        };
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
        result
    };

    if let Some(new_dir) = new_dir_opt {
        // Whole-dir move: re-key every nested cache entry under the new prefix.
        rewrite_dir_album_cache_prefix(&old_dir, &new_dir);
    } else {
        // Merge dissolved the source tree: drop the cache entries for the
        // removed directories (kept, non-empty directories were not removed).
        for dir in removed_dirs {
            evict_dir_album(&dir);
        }
    }

    if let Some(old_parent_id) = get_parent_album_id(&old_dir) {
        mark_album_for_update(old_parent_id);
    }
    mark_album_for_update(target_album_id);

    Ok(outcome)
}

/// `fs::rename` a whole album directory from `source_dir` to `dest_dir`, then
/// rewrite every DB record whose path lived under `source_dir` — the moved
/// album's own `dir_path`, any further-nested sub-albums' `dir_path`, and every
/// image/video alias — to `dest_dir`.
fn rename_whole_dir(
    data_table: &mut redb::Table<'_, &str, AbstractData>,
    source_dir: &Path,
    dest_dir: &Path,
) -> Result<(), AppError> {
    fs::rename(source_dir, dest_dir).map_err(|e| {
        AppError::new(
            ErrorKind::Internal,
            format!("Failed to move album directory: {e}"),
        )
    })?;

    // Collect matches first (immutable iteration) before inserting, since redb
    // doesn't allow mutating a table while iterating it.
    let mut updates: Vec<(ArrayString<64>, AbstractData)> = Vec::new();
    for entry in data_table
        .iter()
        .or_raise(|| (ErrorKind::Database, "Failed to iterate data table"))?
    {
        let (key_guard, val_guard) =
            entry.or_raise(|| (ErrorKind::Database, "Failed to read table entry"))?;
        let key: ArrayString<64> =
            ArrayString::from(key_guard.value()).expect("stored key must fit ArrayString<64>");
        let mut data = val_guard.value();
        if rewrite_paths_under(&mut data, source_dir, dest_dir) {
            updates.push((key, data));
        }
    }
    for (key, data) in updates {
        data_table
            .insert(&*key, data)
            .or_raise(|| (ErrorKind::Database, "Failed to update moved record"))?;
    }
    Ok(())
}

/// Recursive `Merge` when the source directory name collides with an existing
/// target path. Migrates every regular file under `source_dir` up into the
/// target album root, applies per-file merge semantics, then removes the
/// emptied source directories and updates/removes the affected DB records.
///
/// Returns the list of source directories that were physically removed (their
/// dir-album records are deleted and their cache entries evicted by the
/// caller).
fn merge_album_tree(
    data_table: &mut redb::Table<'_, &str, AbstractData>,
    source_dir: &Path,
    target_dir: &Path,
) -> Result<Vec<PathBuf>, AppError> {
    let mut files = Vec::new();
    collect_regular_files(source_dir, &mut files)?;

    // old flattened path -> new path, for records whose alias physically moved.
    let mut moved: HashMap<PathBuf, PathBuf> = HashMap::new();
    // Old alias paths removed by content dedup (alias entries already pruned on
    // disk and recorded here for the DB pass).
    let mut deleted: HashSet<PathBuf> = HashSet::new();

    for src in files {
        let file_name = src
            .file_name()
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Source file has no name"))?;
        let base_dest = target_dir.join(file_name);

        if base_dest.exists() {
            if files_hash_equal(&src, &base_dest)? {
                // Same content: dedup — drop the redundant source copy + its
                // alias entry (pruned in the DB pass via `prune_alias_paths`).
                deleted.insert(src);
                continue;
            }
            // Different content: auto-`-001` suffix, never overwrite.
            let dest = find_unique_path(&base_dest)?;
            rename_file_with_sidecar(&src, &dest)?;
            moved.insert(src, dest);
        } else {
            rename_file_with_sidecar(&src, &base_dest)?;
            moved.insert(src, base_dest);
        }
    }

    let removed_dirs = remove_emptied_dirs(source_dir)?;
    let removed_set: HashSet<PathBuf> = removed_dirs.iter().cloned().collect();
    apply_merge_records(data_table, &moved, &deleted, &removed_set)?;

    Ok(removed_dirs)
}

/// Recursively collect the absolute path of every regular file under `dir`,
/// recursing into subdirectories. `.xmp` sidecars are skipped — a sidecar
/// travels with its media file in `rename_file_with_sidecar`, so collecting it
/// independently would double-move it. A leftover orphan sidecar keeps its
/// directory (and ancestors) non-empty.
fn collect_regular_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), AppError> {
    for entry in std::fs::read_dir(dir).or_raise(|| {
        (
            ErrorKind::Internal,
            format!("Failed to read dir {}", dir.display()),
        )
    })? {
        let entry = entry.or_raise(|| (ErrorKind::Internal, "Failed to read dir entry"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .or_raise(|| (ErrorKind::Internal, "Failed to stat entry"))?;
        if file_type.is_dir() {
            collect_regular_files(&path, out)?;
        } else if file_type.is_file() && path.extension().and_then(|e| e.to_str()) != Some("xmp") {
            out.push(path);
        }
    }
    Ok(())
}

/// Whether two files contain identical bytes, per BLAKE3 content hash.
fn files_hash_equal(a: &Path, b: &Path) -> Result<bool, AppError> {
    let fa = std::fs::File::open(a).map_err(|e| {
        AppError::new(
            ErrorKind::InvalidInput,
            format!("Failed to open source {}: {e}", a.display()),
        )
    })?;
    let fb = std::fs::File::open(b).map_err(|e| {
        AppError::new(
            ErrorKind::InvalidInput,
            format!("Failed to open dest {}: {e}", b.display()),
        )
    })?;
    let ha = crate::process::hash::blake3_hasher(fa).map_err(|e| {
        AppError::new(
            ErrorKind::Internal,
            format!("Failed to hash {}: {e}", a.display()),
        )
    })?;
    let hb = crate::process::hash::blake3_hasher(fb).map_err(|e| {
        AppError::new(
            ErrorKind::Internal,
            format!("Failed to hash {}: {e}", b.display()),
        )
    })?;
    Ok(ha == hb)
}

/// `fs::rename` a media file to `dest` and move its `.xmp` sidecar alongside
/// (best-effort, mirroring `move_item_into_album`). Sidecar move failures are
/// logged, not fatal.
fn rename_file_with_sidecar(src: &Path, dest: &Path) -> Result<(), AppError> {
    fs::rename(src, dest).map_err(|e| {
        AppError::new(
            ErrorKind::Internal,
            format!("Failed to move file {}: {e}", src.display()),
        )
    })?;
    let src_sidecar = src.with_extension("xmp");
    if src_sidecar.exists() {
        let dst_sidecar = dest.with_extension("xmp");
        if let Err(e) = fs::rename(&src_sidecar, &dst_sidecar) {
            warn!("Failed to move XMP sidecar: {e}");
        }
    }
    Ok(())
}

/// Walk every nested directory under `root` from the deepest upward, removing
/// each one that is empty. Stops at the first directory that is still
/// non-empty — keeping it and every ancestor (safety rule). Returns the
/// removed directories (deepest-first).
fn remove_emptied_dirs(root: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut dirs = Vec::new();
    collect_dirs(root, &mut dirs)?;
    // Deepest directories first so a parent is only considered after its
    // children have been removed (making it eligible to become empty).
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));

    let mut removed = Vec::new();
    for dir in dirs {
        if dir_is_empty(&dir)? {
            std::fs::remove_dir(&dir).map_err(|e| {
                AppError::new(
                    ErrorKind::Internal,
                    format!("Failed to remove emptied dir {}: {e}", dir.display()),
                )
            })?;
            removed.push(dir);
        } else {
            break;
        }
    }
    Ok(removed)
}

/// Collect `dir` and every nested directory path (recursively).
fn collect_dirs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), AppError> {
    if dir.is_dir() {
        out.push(dir.to_path_buf());
    }
    for entry in std::fs::read_dir(dir).or_raise(|| {
        (
            ErrorKind::Internal,
            format!("Failed to read dir {}", dir.display()),
        )
    })? {
        let entry = entry.or_raise(|| (ErrorKind::Internal, "Failed to read dir entry"))?;
        let file_type = entry
            .file_type()
            .or_raise(|| (ErrorKind::Internal, "Failed to stat entry"))?;
        if file_type.is_dir() {
            collect_dirs(&entry.path(), out)?;
        }
    }
    Ok(())
}

/// Returns whether `dir` currently contains no entries.
fn dir_is_empty(dir: &Path) -> Result<bool, AppError> {
    Ok(std::fs::read_dir(dir)
        .or_raise(|| {
            (
                ErrorKind::Internal,
                format!("Failed to read dir {}", dir.display()),
            )
        })?
        .next()
        .is_none())
}

/// Persist the effects of a recursive `Merge` to `data_table`: rewrite each
/// moved alias path to its flattened destination, drop alias entries deleted by
/// content-dedup (via `prune_alias_paths`), and remove the dir-album records of
/// every removed source directory. Flattening is not a uniform prefix change,
/// so records are rewritten from the explicit old→new map rather than reusing
/// `rewrite_paths_under`.
fn apply_merge_records(
    data_table: &mut redb::Table<'_, &str, AbstractData>,
    moved: &HashMap<PathBuf, PathBuf>,
    deleted: &HashSet<PathBuf>,
    removed_dirs: &HashSet<PathBuf>,
) -> Result<(), AppError> {
    let mut updates: Vec<(ArrayString<64>, AbstractData)> = Vec::new();
    let mut removals: Vec<ArrayString<64>> = Vec::new();

    for entry in data_table
        .iter()
        .or_raise(|| (ErrorKind::Database, "Failed to iterate data table"))?
    {
        let (key_guard, val_guard) =
            entry.or_raise(|| (ErrorKind::Database, "Failed to read table entry"))?;
        let key: ArrayString<64> =
            ArrayString::from(key_guard.value()).expect("stored key must fit ArrayString<64>");
        let mut data = val_guard.value();

        match &mut data {
            AbstractData::Album(album) => {
                if removed_dirs.contains(Path::new(&album.metadata.dir_path)) {
                    removals.push(key);
                }
            }
            AbstractData::Image(_) | AbstractData::Video(_) => {
                let to_prune: Vec<PathBuf> = data
                    .alias()
                    .iter()
                    .map(|a| PathBuf::from(&a.file))
                    .filter(|p| deleted.contains(p))
                    .collect();
                let mut keep = true;
                for p in &to_prune {
                    keep = crate::process::alias::prune_alias_paths(&mut data, p);
                }
                let mut changed = false;
                if let Some(aliases) = data.alias_mut() {
                    for a in aliases.iter_mut() {
                        let p = PathBuf::from(&a.file);
                        if let Some(new_path) = moved.get(&p) {
                            a.file = new_path.to_string_lossy().into_owned();
                            changed = true;
                        }
                    }
                }
                if !keep {
                    removals.push(key);
                } else if changed || !to_prune.is_empty() {
                    updates.push((key, data));
                }
            }
        }
    }

    for (key, data) in updates {
        data_table
            .insert(&*key, data)
            .or_raise(|| (ErrorKind::Database, "Failed to rewrite moved record"))?;
    }
    for key in removals {
        data_table
            .remove(&*key)
            .or_raise(|| (ErrorKind::Database, "Failed to remove record"))?;
    }
    Ok(())
}

/// Rewrite `data`'s stored path(s) from under `old_prefix` to the equivalent
/// location under `new_prefix`. Returns whether anything changed.
fn rewrite_paths_under(data: &mut AbstractData, old_prefix: &Path, new_prefix: &Path) -> bool {
    match data {
        AbstractData::Album(album) => {
            let dir = PathBuf::from(&album.metadata.dir_path);
            if let Ok(rel) = dir.strip_prefix(old_prefix) {
                album.metadata.dir_path = new_prefix.join(rel).to_string_lossy().into_owned();
                true
            } else {
                false
            }
        }
        AbstractData::Image(_) | AbstractData::Video(_) => {
            let mut changed = false;
            if let Some(alias) = data.alias_mut() {
                for a in alias.iter_mut() {
                    let p = PathBuf::from(&a.file);
                    if let Ok(rel) = p.strip_prefix(old_prefix) {
                        a.file = new_prefix.join(rel).to_string_lossy().into_owned();
                        changed = true;
                    }
                }
            }
            changed
        }
    }
}

/// Determine whether `idx` (the selected alias of `data`) can be merge-deduped
/// into an album-resident alias: another alias of the same record must sit
/// directly under `album_dir`. If a candidate exists, verify the on-disk bytes
/// at the selected path still match the recorded `hash` before the caller
/// prunes anything. Returns `Ok(true)` when dedup should proceed, `Ok(false)`
/// when the caller must fall through to a normal move, and `Err` when the
/// verify fails (source copy left untouched).
fn merge_dedup_candidate(
    data: &AbstractData,
    idx: usize,
    album_dir: &Path,
    current_path: &Path,
    hash: ArrayString<64>,
) -> Result<bool, AppError> {
    let has_candidate = data.alias().iter().enumerate().any(|(i, a)| {
        i != idx && crate::process::alias::normalize_alias_path(&a.file).parent() == Some(album_dir)
    });
    if !has_candidate {
        return Ok(false);
    }

    let file = fs::File::open(current_path).map_err(|e| {
        AppError::new(
            ErrorKind::InvalidInput,
            format!(
                "Failed to open selected alias for verification: {} — {e}",
                current_path.display()
            ),
        )
    })?;
    let actual_hash = crate::process::hash::blake3_hasher(file).map_err(|e| {
        AppError::new(
            ErrorKind::Internal,
            format!("Failed to hash selected alias: {e}"),
        )
    })?;
    if actual_hash != hash {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "merge verify mismatch: on-disk content does not match the recorded hash",
        ));
    }
    Ok(true)
}

#[allow(clippy::too_many_lines)]
fn move_item_into_album(
    hash: ArrayString<64>,
    album_id: ArrayString<64>,
    album_dir: &Path,
    on_conflict: OnConflict,
    selected_alias: Option<String>,
) -> Result<AssignOutcome, AppError> {
    let txn = TREE
        .in_disk
        .begin_write()
        .or_raise(|| (ErrorKind::Database, "Failed to begin write transaction"))?;
    let outcome = {
        let mut data_table = txn
            .open_table(DATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

        let mut abstract_data: AbstractData = data_table
            .get(&*hash)
            .or_raise(|| (ErrorKind::Database, "Failed to look up item"))?
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Item not found in database"))?
            .value();

        let alias = abstract_data.alias();
        if alias.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "Item has no alias"));
        }
        let Some(selected_alias) = selected_alias else {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "alias is required for item records",
            ));
        };
        let norm = crate::process::alias::normalize_alias_path(&selected_alias);
        let idx = alias
            .iter()
            .position(|a| crate::process::alias::normalize_alias_path(&a.file) == norm)
            .ok_or_else(|| {
                AppError::new(
                    ErrorKind::InvalidInput,
                    "alias does not match any alias of this record",
                )
            })?;
        let current_path = PathBuf::from(&alias[idx].file);

        if !current_path.exists() {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!(
                    "File not found at recorded path: {}",
                    current_path.display()
                ),
            ));
        }

        if on_conflict == OnConflict::Merge
            && merge_dedup_candidate(&abstract_data, idx, album_dir, &current_path, hash)?
        {
            let old_album = abstract_data.album();
            crate::process::alias::prune_alias_paths(&mut abstract_data, &current_path);
            abstract_data.set_album(Some(album_id));

            data_table
                .insert(&*hash, abstract_data)
                .or_raise(|| (ErrorKind::Database, "Failed to update item in database"))?;

            if let Some(old_id) = old_album {
                mark_album_for_update(old_id);
            }
            mark_album_for_update(album_id);

            drop(data_table);
            txn.commit()
                .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
            return Ok(AssignOutcome::DeduplicatedRemoved);
        }

        let file_name = current_path
            .file_name()
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias has no filename"))?;
        let base_dest = album_dir.join(file_name);

        let renamed = base_dest.exists() && base_dest != current_path;
        let dest_path = if renamed {
            match on_conflict {
                // Merge with no redundant album-resident alias falls through to
                // a safe rename (matches Rename) — never overwrite an existing
                // album file.
                OnConflict::Rename | OnConflict::Merge => find_unique_path(&base_dest)?,
            }
        } else {
            base_dest
        };

        fs::rename(&current_path, &dest_path)
            .map_err(|e| AppError::new(ErrorKind::Internal, format!("Failed to move file: {e}")))?;

        let src_sidecar = current_path.with_extension("xmp");
        if src_sidecar.exists() {
            let dst_sidecar = dest_path.with_extension("xmp");
            if let Err(e) = fs::rename(&src_sidecar, &dst_sidecar) {
                warn!("Failed to move XMP sidecar: {e}");
            }
        }

        let old_album = abstract_data.album();
        if let Some(alias_mut) = abstract_data.alias_mut() {
            alias_mut[idx].file = dest_path.to_string_lossy().into_owned();
        }
        abstract_data.set_album(Some(album_id));

        data_table
            .insert(&*hash, abstract_data)
            .or_raise(|| (ErrorKind::Database, "Failed to update item in database"))?;

        if let Some(old_id) = old_album {
            mark_album_for_update(old_id);
        }
        mark_album_for_update(album_id);
        if renamed {
            AssignOutcome::RenamedFrom
        } else {
            AssignOutcome::Moved
        }
    };
    txn.commit()
        .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
    Ok(outcome)
}
