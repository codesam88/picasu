use crate::error::{AppError, ErrorKind, ResultExt};
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
pub struct EditRatingData {
    index_array: Vec<usize>,
    timestamp: i64,
    /// Rating value 0–5, or null to clear
    rating: Option<u8>,
}

#[utoipa::path(
        put,
        path = "/put/edit_rating",
        request_body = EditRatingData,
        responses(
            (status = 200, description = "Rating updated"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[put("/put/edit_rating", format = "json", data = "<json_data>")]
pub async fn edit_rating(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<EditRatingData>,
) -> AppResult<Json<()>> {
    let _ = auth?;
    let _ = read_only_mode?;

    if let Some(r) = json_data.rating
        && r > 5
    {
        return Err(crate::error::AppError::new(
            ErrorKind::InvalidInput,
            format!("rating must be 0–5, got {r}"),
        ));
    }

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let data_table = open_data_table();
        let tree_snapshot = open_tree_snapshot_table(json_data.timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

        let mut data_to_flush = Vec::new();

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
                abstract_data.set_rating(json_data.rating);
                data_to_flush.push(abstract_data);
            }
        }

        if !data_to_flush.is_empty() {
            BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(data_to_flush));
        }

        Ok(())
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    let _ = BATCH_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(vec![]))
        .await;
    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    Ok(Json(()))
}
