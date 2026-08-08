use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::process::transitor::index_to_hash;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{open_data_table, open_tree_snapshot_table};
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use rocket::serde::{Deserialize, Serialize, json::Json};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct EditFlagsData {
    index_array: Vec<usize>,
    timestamp: i64,
    #[serde(default)]
    is_favorite: Option<bool>,
    #[serde(default)]
    is_archived: Option<bool>,
}

#[utoipa::path(
        put,
        path = "/put/edit_flags",
        request_body = EditFlagsData,
        responses(
            (status = 200, description = "Flags updated"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/edit_flags", format = "json", data = "<json_data>")]
pub async fn edit_flags(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<EditFlagsData>,
) -> AppResult<Json<()>> {
    let _ = auth?;
    let _ = read_only_mode?;

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let data_table = open_data_table();
        let tree_snapshot = open_tree_snapshot_table(json_data.timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

        let mut data_to_flush: Vec<AbstractData> = Vec::new();

        for &index in &json_data.index_array {
            let hash = index_to_hash(&tree_snapshot, index).or_raise(|| {
                (
                    ErrorKind::Database,
                    format!("Failed to get hash for index {index}"),
                )
            })?;

            if let Some(guard) = data_table
                .get(&*hash)
                .or_raise(|| (ErrorKind::Database, "Failed to get data"))?
            {
                let mut abstract_data = guard.value();

                // Apply flag changes
                if let Some(is_favorite) = json_data.is_favorite {
                    abstract_data.set_favorite(is_favorite);
                }
                if let Some(is_archived) = json_data.is_archived {
                    abstract_data.set_archived(is_archived);
                }

                data_to_flush.push(abstract_data);
            }
        }

        // Flush data
        if !data_to_flush.is_empty() {
            BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(data_to_flush));
        }

        Ok(())
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    // Drain pending flush before rebuilding the in-memory tree.
    let _ = BATCH_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(vec![]))
        .await;
    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    Ok(Json(()))
}
