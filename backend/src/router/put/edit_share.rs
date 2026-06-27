use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::model::album::Share;
use crate::router::GuardResult;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::storage::db::TREE;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::{router::AppResult, storage::db::DATA_TABLE};

use arrayvec::ArrayString;
use redb::ReadableTable;
use rocket::serde::{Deserialize, Serialize, json::Json};
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct EditShare {
    #[schema(value_type = String)]
    album_id: ArrayString<64>,
    share: Share,
}

#[utoipa::path(
        put,
        path = "/put/edit_share",
        request_body = EditShare,
        responses(
            (status = 200, description = "Share updated"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/edit_share", format = "json", data = "<json_data>")]
pub async fn edit_share(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<EditShare>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
        {
            let mut data_table = txn
                .open_table(DATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let album_opt = data_table
                .get(json_data.album_id.as_str())
                .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
                .and_then(|guard| {
                    let abstract_data = guard.value();
                    match abstract_data {
                        AbstractData::Album(album) => Some(album),
                        _ => None,
                    }
                });

            if let Some(mut album) = album_opt {
                album
                    .metadata
                    .share_list
                    .insert(json_data.share.url, json_data.share.clone());
                data_table
                    .insert(json_data.album_id.as_str(), AbstractData::Album(album))
                    .or_raise(|| (ErrorKind::Database, "Failed to update album"))?;
            }
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
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteShare {
    #[schema(value_type = String)]
    album_id: ArrayString<64>,
    #[schema(value_type = String)]
    share_id: ArrayString<64>,
}

#[utoipa::path(
        put,
        path = "/put/delete_share",
        request_body = DeleteShare,
        responses(
            (status = 200, description = "Share deleted"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/delete_share", format = "json", data = "<json_data>")]
pub async fn delete_share(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<DeleteShare>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
        {
            let mut data_table = txn
                .open_table(DATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let album_opt = data_table
                .get(json_data.album_id.as_str())
                .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
                .and_then(|guard| {
                    let abstract_data = guard.value();
                    match abstract_data {
                        AbstractData::Album(album) => Some(album),
                        _ => None,
                    }
                });

            if let Some(mut album) = album_opt {
                album.metadata.share_list.remove(&json_data.share_id);
                data_table
                    .insert(json_data.album_id.as_str(), AbstractData::Album(album))
                    .or_raise(|| (ErrorKind::Database, "Failed to update album"))?;
            }
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
    Ok(())
}
