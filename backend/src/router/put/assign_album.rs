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
use crate::storage::db::METADATA_TABLE;
use crate::storage::db::TREE;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::INDEX_COORDINATOR;
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use arrayvec::ArrayString;
use redb::ReadableTable;
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
    /// Path-primary asset ID. The handler resolves the record via
    /// `ASSET_BY_ID`, allowing independent movement of same-hash files.
    #[schema(value_type = String)]
    pub asset_id: ArrayString<64>,
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
    let asset_id = data.asset_id;
    let album_id = data.album_id;
    let on_conflict = data.on_conflict;
    let selected_alias: Option<String> = data.alias;

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
        move_asset_into_album(
            asset_id,
            album_id,
            &album_dir,
            on_conflict,
            selected_alias.as_deref(),
        )
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

/// Move a single asset (identified by `asset_id`) into `album_dir`.
/// This is the path-primary move: only the one physical file at the asset's
/// canonical path is moved, regardless of hash-matched duplicates.
/// Albums move as directory trees via `move_album_into_album`.
fn move_asset_into_album(
    asset_id: ArrayString<64>,
    album_id: ArrayString<64>,
    album_dir: &Path,
    on_conflict: OnConflict,
    selected_alias: Option<&str>,
) -> Result<AssignOutcome, AppError> {
    use crate::storage::asset_store;

    // Look up the asset record.
    let record = asset_store::get_asset_by_id(&asset_id)
        .or_raise(|| (ErrorKind::Database, "Failed to look up asset"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Asset not found"))?;

    // Albums move as directory trees.
    if record.kind == crate::model::asset::AssetKind::Album {
        #[allow(clippy::needless_option_as_deref)]
        return move_album_into_album(
            asset_id,
            album_id,
            album_dir,
            on_conflict,
            selected_alias.as_deref(),
        );
    }

    let source_path = PathBuf::from(&record.canonical_path);
    if !source_path.exists() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("File not found at: {}", source_path.display()),
        ));
    }

    let file_name = source_path
        .file_name()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "File has no name"))?;
    let base_dest = album_dir.join(file_name);

    let (final_dest, outcome) = if base_dest.exists() {
        if source_path == base_dest {
            return Ok(AssignOutcome::Moved); // already there
        }
        match on_conflict {
            OnConflict::Skip => return Ok(AssignOutcome::Skipped),
            OnConflict::Rename => {
                let unique = crate::process::sanitize::find_unique_path(&base_dest)
                    .or_raise(|| (ErrorKind::IO, "Failed to find unique path"))?;
                (unique, AssignOutcome::RenamedFrom)
            }
        }
    } else {
        (base_dest, AssignOutcome::Moved)
    };

    // Move the file on disk.
    fs::rename(&source_path, &final_dest).or_raise(|| (ErrorKind::IO, "Failed to move file"))?;

    // Move the sidecar if it exists.
    let sidecar = source_path.with_extension("xmp");
    if sidecar.exists() {
        let new_sidecar = final_dest.with_extension("xmp");
        let _ = fs::rename(&sidecar, &new_sidecar);
    }

    // Update the asset record with the new path and album.
    let new_path = final_dest.to_string_lossy().into_owned();
    let mut updated = record.clone();
    updated.canonical_path.clone_from(&new_path);
    updated.album_id = Some(album_id);

    // Update ASSET_BY_ID.
    asset_store::put_asset_by_id(&updated)
        .or_raise(|| (ErrorKind::Database, "Failed to update asset"))?;

    // Update ASSET_BY_PATH: remove old, add new.
    asset_store::remove_asset_by_path(&record.canonical_path)
        .or_raise(|| (ErrorKind::Database, "Failed to remove old path mapping"))?;
    asset_store::put_asset_by_path(&new_path, asset_id)
        .or_raise(|| (ErrorKind::Database, "Failed to add new path mapping"))?;

    // Update METADATA_TABLE so build_from_asset_tables sees the new album.
    {
        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin write transaction"))?;
        {
            let mut metadata_table = txn
                .open_table(METADATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;
            // Extract data first to avoid borrow conflict.
            let existing = metadata_table
                .get(&*asset_id)
                .ok()
                .flatten()
                .map(|guard| guard.value());
            if let Some(mut abstract_data) = existing {
                if let Some(alias) = abstract_data.alias_mut().and_then(|slot| slot.as_mut())
                    && alias.file == record.canonical_path
                {
                    alias.file.clone_from(&new_path);
                }
                abstract_data.set_album(Some(album_id));
                metadata_table
                    .insert(&*asset_id, abstract_data)
                    .or_raise(|| (ErrorKind::Database, "Failed to update METADATA_TABLE"))?;
            }
        }
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
    }

    Ok(outcome)
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
    album_id: ArrayString<64>,
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
            let mut metadata_table = txn
                .open_table(METADATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let abstract_data: AbstractData = metadata_table
                .get(&*album_id)
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
                        drop(metadata_table);
                        txn.commit()
                            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
                        return Ok(AssignOutcome::Skipped);
                    }
                    OnConflict::Rename => {
                        let dest_dir = find_unique_path(&base_dest)?;
                        rename_whole_dir(&mut metadata_table, &source_dir, &dest_dir)?;
                        (source_dir, Some(dest_dir), AssignOutcome::RenamedFrom)
                    }
                }
            } else {
                // No collision: land the whole tree in place (Moved).
                let dest_dir = base_dest;
                rename_whole_dir(&mut metadata_table, &source_dir, &dest_dir)?;
                (source_dir, Some(dest_dir), AssignOutcome::Moved)
            }
        };
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
        result
    };

    if let Some(new_dir) = new_dir_opt {
        rewrite_dir_album_cache_prefix(&old_dir, &new_dir);
        // Update asset tables for all moved files.
        let _ = update_asset_tables_after_dir_move(&old_dir, &new_dir);
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
    metadata_table: &mut redb::Table<'_, &str, AbstractData>,
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
    for entry in metadata_table
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
        metadata_table
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
            if let Some(alias) = data.alias_mut().and_then(|slot| slot.as_mut()) {
                let p = PathBuf::from(&alias.file);
                if let Ok(rel) = p.strip_prefix(old_prefix) {
                    alias.file = new_prefix.join(rel).to_string_lossy().into_owned();
                    changed = true;
                }
            }
            changed
        }
    }
}

/// Update asset tables after a directory move.
/// For each asset whose canonical path starts with `source_dir`, update it
/// to the corresponding path under `dest_dir`.
fn update_asset_tables_after_dir_move(source_dir: &Path, dest_dir: &Path) -> Result<(), AppError> {
    use crate::storage::asset_store;

    let records = asset_store::get_all_assets()
        .or_raise(|| (ErrorKind::Database, "Failed to read asset records"))?;

    for record in records {
        let old_path = PathBuf::from(&record.canonical_path);
        if let Ok(rel) = old_path.strip_prefix(source_dir) {
            let new_path = dest_dir.join(rel);
            let new_path_str = new_path.to_string_lossy().into_owned();

            // Update ASSET_BY_PATH: remove old, add new.
            let _ = asset_store::remove_asset_by_path(&record.canonical_path);
            let _ = asset_store::put_asset_by_path(&new_path_str, record.asset_id);

            // Update canonical_path in ASSET_BY_ID.
            let mut updated = record.clone();
            updated.canonical_path = new_path_str;
            let _ = asset_store::put_asset_by_id(&updated);
        }
    }

    Ok(())
}
