// src/router/delete/mod.rs
use rocket::Route;

pub fn generate_delete_routes() -> Vec<Route> {
    routes![delete_data]
}

// src/router/delete/delete_data.rs
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::model::config::APP_CONFIG;
use crate::process::dir_album::{
    get_parent_album_id, mark_album_for_update, remove_dir_album_from_cache,
    rewrite_dir_album_cache_prefix,
};
use crate::process::transitor::index_to_hash;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::put::assign_album::rewrite_paths_under;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{DATA_TABLE, TREE, open_tree_snapshot_table};
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::{BATCH_COORDINATOR, INDEX_COORDINATOR};
use anyhow::Result;
use arrayvec::ArrayString;
use futures::future::try_join_all;
use log::warn;
use redb::ReadableTable;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteItem {
    pub index: usize,
    pub alias_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteList {
    pub delete_list: Vec<DeleteItem>,
    pub timestamp: i64,
}

#[utoipa::path(
        delete,
        path = "/delete/delete-data",
        request_body = DeleteList,
        responses(
            (status = 200, description = "Data deleted"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[delete("/delete/delete-data", format = "json", data = "<json_data>")]
pub async fn delete_data(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<DeleteList>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;
    let (abstract_data_to_remove, all_affected_album_ids) = tokio::task::spawn_blocking({
        let delete_list = json_data.delete_list.clone();
        let timestamp = json_data.timestamp;
        move || process_deletes(delete_list, timestamp)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::remove(abstract_data_to_remove))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute flush tree task"))?;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute update tree task"))?;

    try_join_all(
        all_affected_album_ids
            .into_iter()
            .map(|album_id| async move {
                INDEX_COORDINATOR
                    .execute_waiting(AlbumSelfUpdateTask::new(album_id))
                    .await
            }),
    )
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to update affected albums"))?;
    Ok(())
}

fn compute_trash_root() -> PathBuf {
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned");
    let image_home = config.image_home.as_ref().expect("image_home not set");
    image_home.join(&config.trash_directory)
}

fn is_in_trash(alias_path: &str, trash_root: &Path) -> bool {
    let path = Path::new(alias_path);
    path.starts_with(trash_root) && path != trash_root
}

fn trash_move_item(
    data_table: &mut redb::Table<&str, AbstractData>,
    hash: &str,
    alias_path: &str,
    trash_root: &Path,
) -> Result<(), AppError> {
    let mut abstract_data: AbstractData = data_table
        .get(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to look up item"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Item not found"))?
        .value();

    let alias_idx = abstract_data
        .alias()
        .iter()
        .position(|a| a.file == alias_path)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias not found"))?;

    let source = Path::new(alias_path);
    let image_home = {
        let config = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned");
        config.image_home.clone().expect("image_home not set")
    };
    let relative = source.strip_prefix(&image_home).unwrap_or(source);
    let dest = trash_root.join(relative);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::new(
                ErrorKind::IO,
                format!("Failed to create trash directory: {e}"),
            )
        })?;
    }
    fs::rename(source, &dest)
        .map_err(|e| AppError::new(ErrorKind::IO, format!("Failed to move file to trash: {e}")))?;

    let src_sidecar = source.with_extension("xmp");
    if src_sidecar.exists() {
        let dst_sidecar = dest.with_extension("xmp");
        if let Err(e) = fs::rename(&src_sidecar, &dst_sidecar) {
            warn!("Failed to move XMP sidecar to trash: {e}");
        }
    }

    if let Some(alias_mut) = abstract_data.alias_mut() {
        alias_mut[alias_idx].file = dest.to_string_lossy().into_owned();
    }
    data_table
        .insert(hash, abstract_data)
        .or_raise(|| (ErrorKind::Database, "Failed to update item alias"))?;
    Ok(())
}

fn trash_move_album(
    data_table: &mut redb::Table<&str, AbstractData>,
    hash: &str,
    trash_root: &Path,
) -> Result<(), AppError> {
    let abstract_data: AbstractData = data_table
        .get(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to look up album"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found"))?
        .value();

    let AbstractData::Album(album) = &abstract_data else {
        return Err(AppError::new(ErrorKind::InvalidInput, "Expected album"));
    };

    let source_dir = PathBuf::from(&album.metadata.dir_path);
    let dir_name = source_dir
        .file_name()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album directory has no name"))?;
    let dest_dir = trash_root.join(dir_name);

    fs::create_dir_all(trash_root).map_err(|e| {
        AppError::new(
            ErrorKind::IO,
            format!("Failed to create trash directory: {e}"),
        )
    })?;
    fs::rename(&source_dir, &dest_dir)
        .map_err(|e| AppError::new(ErrorKind::IO, format!("Failed to move album to trash: {e}")))?;

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
        if rewrite_paths_under(&mut data, &source_dir, &dest_dir) {
            updates.push((key, data));
        }
    }
    for (key, data) in updates {
        data_table
            .insert(&*key, data)
            .or_raise(|| (ErrorKind::Database, "Failed to update moved record"))?;
    }

    rewrite_dir_album_cache_prefix(&source_dir, &dest_dir);

    if let Some(parent_id) = get_parent_album_id(&source_dir) {
        mark_album_for_update(parent_id);
    }

    Ok(())
}

fn permanent_delete_item(
    data_table: &mut redb::Table<&str, AbstractData>,
    hash: &str,
    alias_path: &str,
) -> Result<Option<AbstractData>, AppError> {
    let mut abstract_data: AbstractData = data_table
        .get(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to look up item"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Item not found"))?
        .value();

    let alias_idx = abstract_data
        .alias()
        .iter()
        .position(|a| a.file == alias_path)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias not found"))?;

    let file_path = Path::new(alias_path);
    let _ = fs::remove_file(file_path);
    let sidecar = file_path.with_extension("xmp");
    let _ = fs::remove_file(&sidecar);

    if let Some(alias_mut) = abstract_data.alias_mut() {
        alias_mut.remove(alias_idx);
    }

    if abstract_data.alias().is_empty() {
        let thumb = abstract_data.compressed_path();
        if !thumb.as_os_str().is_empty() {
            let _ = fs::remove_file(&thumb);
        }
        data_table
            .remove(hash)
            .or_raise(|| (ErrorKind::Database, "Failed to remove item record"))?;
        Ok(Some(abstract_data))
    } else {
        data_table
            .insert(hash, abstract_data)
            .or_raise(|| (ErrorKind::Database, "Failed to update item record"))?;
        Ok(None)
    }
}

fn permanent_delete_album(
    data_table: &mut redb::Table<&str, AbstractData>,
    hash: &str,
) -> Result<Vec<AbstractData>, AppError> {
    let abstract_data: AbstractData = data_table
        .get(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to look up album"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found"))?
        .value();

    let AbstractData::Album(album) = &abstract_data else {
        return Err(AppError::new(ErrorKind::InvalidInput, "Expected album"));
    };

    let dir_path = PathBuf::from(&album.metadata.dir_path);
    let mut removed = Vec::new();

    let mut to_remove = Vec::new();
    for entry in data_table
        .iter()
        .or_raise(|| (ErrorKind::Database, "Failed to iterate data table"))?
    {
        let (key_guard, val_guard) =
            entry.or_raise(|| (ErrorKind::Database, "Failed to read table entry"))?;
        let data: AbstractData = val_guard.value();
        let dominated = match &data {
            AbstractData::Album(a) => PathBuf::from(&a.metadata.dir_path).starts_with(&dir_path),
            AbstractData::Image(_) | AbstractData::Video(_) => data
                .alias()
                .iter()
                .any(|a| Path::new(&a.file).starts_with(&dir_path)),
        };
        if dominated {
            let key: ArrayString<64> =
                ArrayString::from(key_guard.value()).expect("stored key must fit ArrayString<64>");
            to_remove.push((key, data));
        }
    }

    for (key, data) in to_remove {
        match &data {
            AbstractData::Album(_) => {}
            AbstractData::Image(_) | AbstractData::Video(_) => {
                for alias in data.alias() {
                    let _ = fs::remove_file(&alias.file);
                    let _ = fs::remove_file(Path::new(&alias.file).with_extension("xmp"));
                }
                let thumb = data.compressed_path();
                if !thumb.as_os_str().is_empty() {
                    let _ = fs::remove_file(&thumb);
                }
            }
        }
        data_table.remove(&*key).or_raise(|| {
            (
                ErrorKind::Database,
                format!("Failed to remove record {key}"),
            )
        })?;
        removed.push(data);
    }

    data_table
        .remove(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to remove album record"))?;
    removed.push(abstract_data);

    let _ = fs::remove_dir_all(&dir_path);

    rewrite_dir_album_cache_prefix(&dir_path, &PathBuf::from(""));

    Ok(removed)
}

/// Walk upward from `leaf_dir`, removing album records and empty directories
/// at each level. Stops at the first non-empty directory.
///
/// For each directory visited:
/// - If it doesn't exist on disk: remove the stale album DB record + cache entry.
/// - If it exists but is empty (after removing `.albuminfo.xmp` if present):
///   remove album DB record, directory, and cache entry.
/// - If it has other contents: stop.
pub fn purge_empty_albums(
    leaf_dir: &Path,
    data_table: &mut redb::Table<&str, AbstractData>,
) -> Result<(), AppError> {
    let mut evicted = Vec::new();
    let mut current = Some(leaf_dir.to_path_buf());

    while let Some(dir) = current {
        if !dir.is_dir() {
            remove_album_for_dir(data_table, &dir)?;
            evicted.push(dir.clone());
            current = dir.parent().map(Path::to_path_buf);
            continue;
        }

        let sidecar = dir.join(".albuminfo.xmp");
        if sidecar.exists() {
            let _ = fs::remove_file(&sidecar);
        }

        let is_empty = fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_none());

        if is_empty {
            remove_album_for_dir(data_table, &dir)?;
            let _ = fs::remove_dir(&dir);
            evicted.push(dir.clone());
            current = dir.parent().map(Path::to_path_buf);
        } else {
            break;
        }
    }

    for path in evicted {
        remove_dir_album_from_cache(&path);
    }

    Ok(())
}

/// Find and remove the album DB record whose `dir_path` matches `dir`.
fn remove_album_for_dir(
    data_table: &mut redb::Table<&str, AbstractData>,
    dir: &Path,
) -> Result<(), AppError> {
    let dir_str = dir.to_string_lossy().into_owned();
    let mut to_remove: Vec<ArrayString<64>> = Vec::new();

    for entry in data_table
        .iter()
        .or_raise(|| (ErrorKind::Database, "Failed to iterate data table"))?
    {
        let (key_guard, val_guard) =
            entry.or_raise(|| (ErrorKind::Database, "Failed to read table entry"))?;
        if let AbstractData::Album(album) = val_guard.value()
            && album.metadata.dir_path == dir_str
        {
            let key: ArrayString<64> =
                ArrayString::from(key_guard.value()).expect("stored key must fit ArrayString<64>");
            to_remove.push(key);
        }
    }

    for key in to_remove {
        data_table.remove(&*key).or_raise(|| {
            (
                ErrorKind::Database,
                format!("Failed to remove album record {key}"),
            )
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn is_in_trash_returns_true_for_path_under_trash_root() {
        let trash_root = PathBuf::from("/images/.trash");
        assert!(is_in_trash("/images/.trash/photos/foo.jpg", &trash_root));
    }

    #[test]
    fn is_in_trash_returns_false_for_path_outside_trash() {
        let trash_root = PathBuf::from("/images/.trash");
        assert!(!is_in_trash("/images/photos/foo.jpg", &trash_root));
    }

    #[test]
    fn is_in_trash_returns_false_for_partial_prefix_match() {
        let trash_root = PathBuf::from("/images/.trash");
        assert!(!is_in_trash("/images/.trashy/foo.jpg", &trash_root));
    }

    #[test]
    fn is_in_trash_returns_false_for_exact_trash_root() {
        let trash_root = PathBuf::from("/images/.trash");
        assert!(!is_in_trash("/images/.trash", &trash_root));
    }

    #[test]
    fn is_in_trash_handles_relative_paths() {
        let trash_root = PathBuf::from(".trash");
        assert!(is_in_trash(".trash/foo.jpg", &trash_root));
        assert!(!is_in_trash("photos/foo.jpg", &trash_root));
    }

    #[test]
    fn is_in_trash_handles_nested_trash_path() {
        let trash_root = PathBuf::from("/images/.trash");
        assert!(is_in_trash("/images/.trash/a/b/c.jpg", &trash_root));
    }
}

fn process_deletes(
    delete_list: Vec<DeleteItem>,
    timestamp: i64,
) -> Result<(Vec<AbstractData>, Vec<ArrayString<64>>), AppError> {
    let tree_snapshot = open_tree_snapshot_table(timestamp)
        .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

    let trash_root = compute_trash_root();
    let trash_enabled = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .trash_enabled;

    // Resolve all indices to hashes using the read-only snapshot
    let mut items: Vec<(ArrayString<64>, String)> = Vec::with_capacity(delete_list.len());
    for item in delete_list {
        let hash = index_to_hash(&tree_snapshot, item.index).or_raise(|| {
            (
                ErrorKind::Database,
                format!("Failed to resolve index {}", item.index),
            )
        })?;
        items.push((hash, item.alias_path));
    }

    let mut all_affected_album_ids = Vec::new();
    let mut abstract_data_to_remove = Vec::new();
    let mut purge_leaf_dirs: BTreeSet<PathBuf> = BTreeSet::new();

    let txn = TREE
        .in_disk
        .begin_write()
        .or_raise(|| (ErrorKind::Database, "Failed to begin write transaction"))?;
    {
        let mut data_table = txn
            .open_table(DATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

        for (hash, alias_path) in items {
            let abstract_data: AbstractData = data_table
                .get(&*hash)
                .or_raise(|| (ErrorKind::Database, "Failed to look up item"))?
                .ok_or_else(|| {
                    AppError::new(
                        ErrorKind::InvalidInput,
                        format!("Item not found for hash {hash}"),
                    )
                })?
                .value();

            let affected_albums = match &abstract_data {
                AbstractData::Image(img) => img.metadata.album.iter().copied().collect(),
                AbstractData::Video(vid) => vid.metadata.album.iter().copied().collect(),
                AbstractData::Album(alb) => vec![alb.object.id],
            };

            // Determine action: trash-move or permanent-delete
            let should_trash = trash_enabled && !is_in_trash(&alias_path, &trash_root);

            match &abstract_data {
                AbstractData::Album(album) => {
                    if should_trash {
                        trash_move_album(&mut data_table, &hash, &trash_root)?;
                    } else {
                        let dir_path = PathBuf::from(&album.metadata.dir_path);
                        let removed = permanent_delete_album(&mut data_table, &hash)?;
                        abstract_data_to_remove.extend(removed);
                        if let Some(parent) = dir_path.parent() {
                            purge_leaf_dirs.insert(parent.to_path_buf());
                        }
                    }
                }
                AbstractData::Image(_) | AbstractData::Video(_) => {
                    if should_trash {
                        trash_move_item(&mut data_table, &hash, &alias_path, &trash_root)?;
                    } else {
                        let removed = permanent_delete_item(&mut data_table, &hash, &alias_path)?;
                        if let Some(data) = removed {
                            abstract_data_to_remove.push(data);
                        }
                        if let Some(parent) = Path::new(&alias_path).parent() {
                            purge_leaf_dirs.insert(parent.to_path_buf());
                        }
                    }
                }
            }

            all_affected_album_ids.extend(affected_albums);
        }

        // Purge empty albums left behind by permanent deletes (deepest-first
        // so child dirs are handled before parents).
        for leaf in purge_leaf_dirs.iter().rev() {
            purge_empty_albums(leaf, &mut data_table)?;
        }
    }
    txn.commit()
        .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;

    Ok((abstract_data_to_remove, all_affected_album_ids))
}
