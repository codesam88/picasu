use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::model::config::APP_CONFIG;
use crate::model::response::FileModify;
use crate::process::dir_album::{
    get_parent_album_id, mark_album_for_update, rewrite_dir_album_cache_prefix,
};
use crate::process::transitor::index_to_hash;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::put::assign_album::rewrite_paths_under;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{DATA_TABLE, TREE, open_tree_snapshot_table};
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::{BATCH_COORDINATOR, INDEX_COORDINATOR};
use anyhow::Result;
use arrayvec::ArrayString;
use log::warn;
use redb::ReadableTable;
use rocket::serde::{Deserialize, json::Json};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreItem {
    pub index: usize,
    pub alias_path: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreList {
    pub restore_list: Vec<RestoreItem>,
    pub timestamp: i64,
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

fn compute_image_home() -> PathBuf {
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned");
    config.image_home.clone().expect("image_home not set")
}

#[utoipa::path(
        put,
        path = "/put/restore-from-trash",
        request_body = RestoreList,
        responses(
            (status = 200, description = "Items restored from trash"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/restore-from-trash", format = "json", data = "<json_data>")]
pub async fn restore_from_trash(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<RestoreList>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;

    let restore_list = json_data.into_inner();
    let all_affected_album_ids =
        tokio::task::spawn_blocking(move || process_restores(restore_list))
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    for album_id in all_affected_album_ids {
        INDEX_COORDINATOR
            .execute_waiting(AlbumSelfUpdateTask::new(album_id))
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to update album stats"))?
            .map_err(|e| AppError::new(ErrorKind::Internal, format!("Album update failed: {e}")))?;
    }

    Ok(())
}

fn process_restores(restore_list: RestoreList) -> Result<Vec<ArrayString<64>>, AppError> {
    let tree_snapshot = open_tree_snapshot_table(restore_list.timestamp)
        .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

    let trash_root = compute_trash_root();
    let image_home = compute_image_home();

    let mut items: Vec<(ArrayString<64>, String)> =
        Vec::with_capacity(restore_list.restore_list.len());
    for item in restore_list.restore_list {
        let hash = index_to_hash(&tree_snapshot, item.index).or_raise(|| {
            (
                ErrorKind::Database,
                format!("Failed to resolve index {}", item.index),
            )
        })?;
        items.push((hash, item.alias_path));
    }

    let mut all_affected_album_ids = Vec::new();

    let txn = TREE
        .in_disk
        .begin_write()
        .or_raise(|| (ErrorKind::Database, "Failed to begin write transaction"))?;
    {
        let mut data_table = txn
            .open_table(DATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

        for (hash, alias_path) in items {
            let alias_path = PathBuf::from(&alias_path);

            if !alias_path.starts_with(&trash_root) {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Path {} is not under trash root {}",
                        alias_path.display(),
                        trash_root.display()
                    ),
                ));
            }

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

            match &abstract_data {
                AbstractData::Album(_) => {
                    restore_album(
                        &mut data_table,
                        &hash,
                        &alias_path,
                        &trash_root,
                        &image_home,
                    )?;
                    if let Some(album_id) = get_album_id_from_hash(&data_table, &hash) {
                        all_affected_album_ids.push(album_id);
                    }
                }
                AbstractData::Image(_) | AbstractData::Video(_) => {
                    restore_item(
                        &mut data_table,
                        &hash,
                        &alias_path,
                        &trash_root,
                        &image_home,
                    )?;
                }
            }
        }
    }
    txn.commit()
        .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;

    Ok(all_affected_album_ids)
}

fn get_album_id_from_hash(
    data_table: &redb::Table<&str, AbstractData>,
    hash: &str,
) -> Option<ArrayString<64>> {
    let abstract_data: AbstractData = data_table.get(hash).ok().flatten().map(|v| v.value())?;

    match abstract_data {
        AbstractData::Album(album) => Some(album.object.id),
        AbstractData::Image(img) => img.metadata.album,
        AbstractData::Video(vid) => vid.metadata.album,
    }
}

fn restore_item(
    data_table: &mut redb::Table<&str, AbstractData>,
    hash: &str,
    alias_path: &Path,
    trash_root: &Path,
    image_home: &Path,
) -> Result<(), AppError> {
    let mut abstract_data: AbstractData = data_table
        .get(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to look up item"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Item not found"))?
        .value();

    let alias_idx = abstract_data
        .alias()
        .iter()
        .position(|a| a.file == alias_path.to_string_lossy())
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Alias not found"))?;

    let relative = alias_path.strip_prefix(trash_root).map_err(|e| {
        AppError::new(
            ErrorKind::InvalidInput,
            format!("Failed to strip trash prefix: {e}"),
        )
    })?;
    let dest = image_home.join(relative);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::new(
                ErrorKind::IO,
                format!("Failed to create destination directory: {e}"),
            )
        })?;
    }

    if !alias_path.exists() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("File not found at trash path: {}", alias_path.display()),
        ));
    }

    fs::rename(alias_path, &dest)
        .map_err(|e| AppError::new(ErrorKind::IO, format!("Failed to restore file: {e}")))?;

    let src_sidecar = alias_path.with_extension("xmp");
    if src_sidecar.exists() {
        let dst_sidecar = dest.with_extension("xmp");
        if let Err(e) = fs::rename(&src_sidecar, &dst_sidecar) {
            warn!("Failed to restore XMP sidecar: {e}");
        }
    }

    let modified = abstract_data.alias()[alias_idx].modified;
    let scan_time = abstract_data.alias()[alias_idx].scan_time;
    if let Some(alias_mut) = abstract_data.alias_mut() {
        *alias_mut = vec![FileModify {
            file: dest.to_string_lossy().into_owned(),
            modified,
            scan_time,
        }];
    }

    data_table
        .insert(hash, abstract_data)
        .or_raise(|| (ErrorKind::Database, "Failed to update item in database"))?;

    Ok(())
}

fn restore_album(
    data_table: &mut redb::Table<&str, AbstractData>,
    hash: &str,
    _alias_path: &Path,
    trash_root: &Path,
    image_home: &Path,
) -> Result<(), AppError> {
    let abstract_data: AbstractData = data_table
        .get(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to look up album"))?
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Album not found"))?
        .value();

    let AbstractData::Album(album) = &abstract_data else {
        return Err(AppError::new(ErrorKind::InvalidInput, "Expected an album"));
    };

    let source_dir = PathBuf::from(&album.metadata.dir_path);
    if !source_dir.is_dir() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!(
                "Album directory not found at trash path: {}",
                source_dir.display()
            ),
        ));
    }

    let relative = source_dir.strip_prefix(trash_root).map_err(|e| {
        AppError::new(
            ErrorKind::InvalidInput,
            format!("Failed to strip trash prefix from album dir: {e}"),
        )
    })?;
    let dest_dir = image_home.join(relative);

    if let Some(parent) = dest_dir.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::new(
                ErrorKind::IO,
                format!("Failed to create destination directory: {e}"),
            )
        })?;
    }

    fs::rename(&source_dir, &dest_dir).map_err(|e| {
        AppError::new(
            ErrorKind::IO,
            format!("Failed to restore album directory: {e}"),
        )
    })?;

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
            .or_raise(|| (ErrorKind::Database, "Failed to update restored record"))?;
    }

    rewrite_dir_album_cache_prefix(&source_dir, &dest_dir);

    if let Some(parent_id) = get_parent_album_id(&dest_dir) {
        mark_album_for_update(parent_id);
    }

    Ok(())
}
