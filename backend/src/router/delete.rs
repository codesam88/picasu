// src/router/delete/mod.rs
use rocket::Route;

pub fn generate_delete_routes() -> Vec<Route> {
    routes![delete_data]
}

// src/router/delete/delete_data.rs
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::process::alias::{normalize_alias_path, prune_alias_paths};
use crate::process::dir_album::evict_dir_album;
use crate::process::transitor::index_to_abstract_data;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{open_data_table, open_tree_snapshot_table};
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::{BATCH_COORDINATOR, INDEX_COORDINATOR};
use anyhow::Result;
use arrayvec::ArrayString;
use futures::future::try_join_all;
use log::warn;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteList {
    #[allow(clippy::struct_field_names)]
    delete_list: Vec<usize>,
    #[serde(default)]
    alias_list: Vec<Option<String>>,
    timestamp: i64,
}

type DeleteResult = (Vec<AbstractData>, Vec<AbstractData>, Vec<ArrayString<64>>);

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

    if !json_data.alias_list.is_empty() && json_data.alias_list.len() != json_data.delete_list.len()
    {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "aliasList length must equal deleteList length",
        ));
    }

    let (abstract_data_to_remove, abstract_data_to_update, all_affected_album_ids) =
        tokio::task::spawn_blocking({
            let delete_list = json_data.delete_list.clone();
            let alias_list = json_data.alias_list.clone();
            let timestamp = json_data.timestamp;
            move || process_deletes(&delete_list, &alias_list, timestamp)
        })
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    if !abstract_data_to_remove.is_empty() {
        BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::remove(abstract_data_to_remove))
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to execute flush tree task"))?;
    }

    if !abstract_data_to_update.is_empty() {
        BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::insert(abstract_data_to_update))
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to execute insert tree task"))?;
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

fn process_deletes(
    delete_list: &[usize],
    alias_list: &[Option<String>],
    timestamp: i64,
) -> Result<DeleteResult, AppError> {
    let data_table = open_data_table();
    let tree_snapshot = open_tree_snapshot_table(timestamp)
        .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

    let use_alias_list = !alias_list.is_empty();
    let mut all_affected_album_ids = Vec::new();
    let mut abstract_data_to_remove = Vec::new();
    let mut abstract_data_to_update = Vec::new();

    for (i, index) in delete_list.iter().enumerate() {
        let mut abstract_data = index_to_abstract_data(&tree_snapshot, &data_table, *index)
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    format!("Failed to retrieve data at index {index}"),
                )
            })?;

        let affected_albums = match &abstract_data {
            AbstractData::Image(img) => img.metadata.album.iter().copied().collect(),
            AbstractData::Video(vid) => vid.metadata.album.iter().copied().collect(),
            AbstractData::Album(alb) => {
                if !alb.metadata.dir_path.is_empty() {
                    evict_dir_album(Path::new(&alb.metadata.dir_path));
                }
                vec![alb.object.id]
            }
        };

        if use_alias_list {
            if let Some(target_alias) = &alias_list[i] {
                if matches!(abstract_data, AbstractData::Album(_)) {
                    return Err(AppError::new(
                        ErrorKind::InvalidInput,
                        "aliasList entry must be null for album records",
                    ));
                }

                let target_path = normalize_alias_path(target_alias);

                let has_alias = abstract_data
                    .alias()
                    .iter()
                    .any(|a| normalize_alias_path(&a.file) == target_path);
                if !has_alias {
                    return Err(AppError::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "aliasList entry does not match any alias of the record at index {index}"
                        ),
                    ));
                }

                let remaining = prune_alias_paths(&mut abstract_data, &target_path);

                all_affected_album_ids.extend(affected_albums);
                if remaining {
                    abstract_data_to_update.push(abstract_data);
                } else {
                    abstract_data_to_remove.push(abstract_data);
                }
            } else {
                // null entry: full record removal (album or explicit null).
                all_affected_album_ids.extend(affected_albums);
                abstract_data_to_remove.push(abstract_data);
            }
        } else {
            // Legacy path: aliasList not provided — remove entire record
            // including all alias files and sidecars from disk.
            for alias in abstract_data.alias() {
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
            let thumb = abstract_data.compressed_path();
            if thumb.exists()
                && let Err(e) = std::fs::remove_file(&thumb)
            {
                warn!("Failed to delete thumbnail {}: {e}", thumb.display());
            }
            all_affected_album_ids.extend(affected_albums);
            abstract_data_to_remove.push(abstract_data);
        }
    }

    Ok((
        abstract_data_to_remove,
        abstract_data_to_update,
        all_affected_album_ids,
    ))
}
