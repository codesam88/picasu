// src/router/delete/mod.rs
use rocket::Route;

pub fn generate_delete_routes() -> Vec<Route> {
    routes![delete_data]
}

// src/router/delete/delete_data.rs
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::process::dir_album::evict_dir_album;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{METADATA_TABLE, TREE};
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::{BATCH_COORDINATOR, INDEX_COORDINATOR};
use anyhow::Result;
use arrayvec::ArrayString;
use futures::future::try_join_all;
use log::warn;
use redb::ReadableDatabase;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteList {
    /// Asset IDs to delete. Each asset is resolved via `METADATA_TABLE` by its
    /// `asset_id` key. The canonical file and sidecar are removed from disk.
    asset_ids: Vec<String>,
    timestamp: i64,
}

type DeleteResult = (Vec<AbstractData>, Vec<ArrayString<64>>);

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

    if json_data.asset_ids.is_empty() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "assetIds must not be empty",
        ));
    }

    let (abstract_data_to_remove, all_affected_album_ids) = tokio::task::spawn_blocking({
        let asset_ids = json_data.asset_ids.clone();
        let timestamp = json_data.timestamp;
        move || process_deletes(&asset_ids, timestamp)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    // Recursive album cleanup: for each album being removed, clean up
    // descendant files, sidecars, asset tables, and directories.
    // This runs after process_deletes validates all entries.
    tokio::task::spawn_blocking({
        let to_remove = abstract_data_to_remove.clone();
        move || cleanup_album_descendants(&to_remove)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?;

    if !abstract_data_to_remove.is_empty() {
        BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::remove(abstract_data_to_remove))
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to execute flush tree task"))?;
    }

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

/// Recursively clean up descendant assets for any albums in the removal list.
/// Removes files, sidecars, asset table entries, and directories from disk.
fn cleanup_album_descendants(abstract_data_to_remove: &[AbstractData]) {
    for abstract_data in abstract_data_to_remove {
        if let AbstractData::Album(alb) = abstract_data {
            if alb.metadata.dir_path.is_empty() {
                continue;
            }
            let dir_path = Path::new(&alb.metadata.dir_path);

            // Find and remove all descendant assets.
            if let Ok(descendants) =
                crate::storage::asset_store::get_assets_under_path(&alb.metadata.dir_path)
            {
                for desc in &descendants {
                    // Delete file + sidecar from disk.
                    let file_path = Path::new(&desc.canonical_path);
                    if let Err(e) = std::fs::remove_file(file_path)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        warn!("Failed to delete descendant {}: {e}", file_path.display());
                    }
                    let sidecar = file_path.with_extension("xmp");
                    if sidecar.exists()
                        && let Err(e) = std::fs::remove_file(&sidecar)
                    {
                        warn!(
                            "Failed to delete descendant sidecar {}: {e}",
                            sidecar.display()
                        );
                    }

                    // Clean up asset tables.
                    let _ = crate::storage::asset_store::remove_asset(desc);

                    // Evict child album caches.
                    if desc.kind == crate::model::asset::AssetKind::Album {
                        evict_dir_album(Path::new(&desc.canonical_path));
                    }
                }

                // Flush descendant records from METADATA_TABLE.
                let desc_abstract: Vec<AbstractData> = descendants
                    .iter()
                    .map(crate::process::transitor::asset_record_to_abstract_data)
                    .collect();
                if !desc_abstract.is_empty() {
                    // Use blocking wait to ensure METADATA_TABLE is updated before we proceed.
                    let _ = futures::executor::block_on(
                        BATCH_COORDINATOR
                            .execute_batch_waiting(FlushTreeTask::remove(desc_abstract)),
                    );
                }
            }

            // Remove the directory tree itself.
            let _ = std::fs::remove_dir_all(dir_path);
        }
    }
}

/// Process deletions by asset ID.
///
/// For each `asset_id`:
/// 1. Look up the `AbstractData` in `METADATA_TABLE` by `asset_id` key.
/// 2. Delete the canonical file + sidecar from disk.
/// 3. Remove the compressed thumbnail only if no other assets share the
///    same content hash (checked via `DUPE_INDEX`).
/// 4. Collect affected album IDs for later self-update.
fn process_deletes(asset_ids: &[String], _timestamp: i64) -> Result<DeleteResult, AppError> {
    let txn = TREE
        .in_disk
        .begin_read()
        .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
    let metadata_table = txn
        .open_table(METADATA_TABLE)
        .or_raise(|| (ErrorKind::Database, "Failed to open METADATA_TABLE"))?;

    let mut all_affected_album_ids = Vec::new();
    let mut abstract_data_to_remove = Vec::new();

    for asset_id_str in asset_ids {
        let asset_id: ArrayString<64> = ArrayString::from(asset_id_str.as_str()).map_err(|_| {
            AppError::new(
                ErrorKind::InvalidInput,
                format!("Invalid asset_id format: {asset_id_str}"),
            )
        })?;

        let abstract_data: AbstractData = metadata_table
            .get(&*asset_id)
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    format!("Failed to look up asset {asset_id}"),
                )
            })?
            .ok_or_else(|| {
                AppError::new(ErrorKind::NotFound, format!("Asset not found: {asset_id}"))
            })?
            .value();

        // Collect affected albums.
        let affected_albums: Vec<ArrayString<64>> = match &abstract_data {
            AbstractData::Image(img) => img.metadata.album.iter().copied().collect(),
            AbstractData::Video(vid) => vid.metadata.album.iter().copied().collect(),
            AbstractData::Album(alb) => {
                if !alb.metadata.dir_path.is_empty() {
                    evict_dir_album(Path::new(&alb.metadata.dir_path));
                }
                vec![alb.object.id]
            }
        };

        // Delete canonical file + sidecar from disk.
        if let Some(alias) = abstract_data.alias() {
            let original = Path::new(&alias.file);
            if let Err(e) = std::fs::remove_file(original)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                warn!("Failed to delete file {}: {e}", original.display());
            }
            let sidecar = original.with_extension("xmp");
            if sidecar.exists()
                && let Err(e) = std::fs::remove_file(&sidecar)
            {
                warn!("Failed to delete sidecar {}: {e}", sidecar.display());
            }
        }

        // Only remove thumbnail if no other assets share this content hash.
        let content_hash = abstract_data.hash();
        let other_refs = match crate::storage::asset_store::get_dupe_ids(&content_hash) {
            Ok(ids) => ids.len(),
            Err(_) => 0,
        };
        if other_refs <= 1 {
            let thumb = abstract_data.compressed_path();
            if thumb.exists()
                && let Err(e) = std::fs::remove_file(&thumb)
            {
                warn!("Failed to delete thumbnail {}: {e}", thumb.display());
            }
        }

        all_affected_album_ids.extend(affected_albums);
        abstract_data_to_remove.push(abstract_data);
    }

    Ok((abstract_data_to_remove, all_affected_album_ids))
}
