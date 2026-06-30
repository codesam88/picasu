use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::response::FileModify;
use crate::process::dir_album::{get_dir_path_for_album, mark_album_for_update};
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::INDEX_COORDINATOR;
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use arrayvec::ArrayString;
use log::warn;
use redb::ReadableTable;
use rocket::serde::{Deserialize, json::Json};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignAlbumData {
    #[schema(value_type = String)]
    pub hash: ArrayString<64>,
    #[schema(value_type = String)]
    pub album_id: ArrayString<64>,
}

/// Move a media item into the album's directory on disk, update the DB alias,
/// and record the explicit album membership.  Returns 422 if the file is not
/// found at the recorded alias path (stale alias — user must re-index first).
#[utoipa::path(
        put,
        path = "/put/assign_album",
        request_body = AssignAlbumData,
        responses(
            (status = 200, description = "Item assigned to album"),
            (status = 400, description = "Invalid input or item not found"),
        )
    )
]
#[put("/put/assign_album", format = "json", data = "<json_data>")]
pub async fn assign_album(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<AssignAlbumData>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;

    let data = json_data.into_inner();
    let hash = data.hash;
    let album_id = data.album_id;

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

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin write transaction"))?;
        {
            let mut data_table = txn
                .open_table(DATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let mut abstract_data = data_table
                .get(&*hash)
                .or_raise(|| (ErrorKind::Database, "Failed to look up item"))?
                .ok_or_else(|| {
                    AppError::new(ErrorKind::InvalidInput, "Item not found in database")
                })?
                .value();

            let alias = abstract_data.alias();
            if alias.is_empty() {
                return Err(AppError::new(ErrorKind::InvalidInput, "Item has no alias"));
            }
            let current_path = PathBuf::from(&alias[0].file);

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
            let dest_path = album_dir.join(file_name);

            fs::rename(&current_path, &dest_path).map_err(|e| {
                AppError::new(ErrorKind::Internal, format!("Failed to move file: {e}"))
            })?;

            // Move sidecar alongside the original if it exists.
            let src_sidecar = current_path.with_extension("xmp");
            if src_sidecar.exists() {
                let dst_sidecar = dest_path.with_extension("xmp");
                if let Err(e) = fs::rename(&src_sidecar, &dst_sidecar) {
                    warn!("Failed to move XMP sidecar: {e}");
                }
            }

            // Record previous album so it can be marked for update.
            let old_album = abstract_data.album();

            // Update alias to new path.
            let modified = alias[0].modified;
            let scan_time = alias[0].scan_time;
            if let Some(alias_mut) = abstract_data.alias_mut() {
                *alias_mut = vec![FileModify {
                    file: dest_path.to_string_lossy().into_owned(),
                    modified,
                    scan_time,
                }];
            }

            // Record explicit album membership.
            abstract_data.set_album(Some(album_id));

            data_table
                .insert(&*hash, abstract_data)
                .or_raise(|| (ErrorKind::Database, "Failed to update item in database"))?;

            // Mark old album for stats refresh.
            if let Some(old_id) = old_album {
                mark_album_for_update(old_id);
            }
            mark_album_for_update(album_id);
        }
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
        Ok(())
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    INDEX_COORDINATOR
        .execute_waiting(AlbumSelfUpdateTask::new(album_id))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update album stats"))?
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("Album update failed: {e}")))?;

    Ok(())
}
