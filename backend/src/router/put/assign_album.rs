use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;

use crate::process::dir_album::{
    get_dir_path_for_album, get_parent_album_id, mark_album_for_update,
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
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, utoipa::ToSchema, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum OnConflict {
    Skip,
    Rename,
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
/// auto-`-001` suffix collision), or `skipped` (destination already exists and
/// strategy is skip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AssignOutcome {
    Moved,
    RenamedFrom,
    Skipped,
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
/// path, the whole tree is renamed to `target_dir/<name>/...` and every record
/// under the old prefix is rewritten. The physical `fs::rename` carries each
/// file's `.xmp` sidecar along, so only the stored path *strings* need
/// updating.
///
/// On a name collision (`base_dest` already exists):
/// - `Skip` leaves both source and target untouched, reported as `Skipped`.
/// - `Rename` renames the whole directory to a unique `-001` sibling
///   (`find_unique_path`) with the same path-rewrite, reported as
///   `RenamedFrom`.
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
    let (old_dir, new_dir_opt, outcome) = {
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
                    OnConflict::Skip => {
                        drop(data_table);
                        txn.commit()
                            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
                        return Ok(AssignOutcome::Skipped);
                    }
                    OnConflict::Rename => {
                        let dest_dir = find_unique_path(&base_dest)?;
                        rename_whole_dir(&mut data_table, &source_dir, &dest_dir)?;
                        (source_dir, Some(dest_dir), AssignOutcome::RenamedFrom)
                    }
                }
            } else {
                // No collision: land the whole tree in place (Moved).
                let dest_dir = base_dest;
                rename_whole_dir(&mut data_table, &source_dir, &dest_dir)?;
                (source_dir, Some(dest_dir), AssignOutcome::Moved)
            }
        };
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
        result
    };

    if let Some(new_dir) = new_dir_opt {
        rewrite_dir_album_cache_prefix(&old_dir, &new_dir);
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

        let file_name = current_path
            .file_name()
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias has no filename"))?;
        let base_dest = album_dir.join(file_name);

        let renamed = base_dest.exists() && base_dest != current_path;
        let dest_path = if renamed {
            match on_conflict {
                OnConflict::Skip => {
                    drop(data_table);
                    txn.commit()
                        .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
                    return Ok(AssignOutcome::Skipped);
                }
                OnConflict::Rename => find_unique_path(&base_dest)?,
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
