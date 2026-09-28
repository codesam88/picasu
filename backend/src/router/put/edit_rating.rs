use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::process::sidecar_edit::{EditedItem, commit_metadata_edits};
use crate::process::transitor::{compose_by_asset_id, index_to_asset_id, store_metadata_record};
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::open_tree_snapshot_table;
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
        let tree_snapshot = open_tree_snapshot_table(json_data.timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

        let mut edited: Vec<EditedItem> = Vec::new();

        for &index in &json_data.index_array {
            let asset_id = index_to_asset_id(&tree_snapshot, index).or_raise(|| {
                (
                    ErrorKind::Database,
                    format!("Failed to get asset_id for index {index}"),
                )
            })?;

            if let Some(mut abstract_data) = compose_by_asset_id(&asset_id)
                .or_raise(|| (ErrorKind::Database, "Failed to get data"))?
            {
                abstract_data.set_rating(json_data.rating);
                edited.push(EditedItem {
                    asset_id,
                    data: abstract_data,
                });
            }
        }

        // Sidecars first, payloads second: a rating that reached only the
        // cache would be reverted by the next reindex, silently.
        commit_metadata_edits(&edited, |asset_id, data: &AbstractData| {
            store_metadata_record(asset_id, data, None)
                .or_raise(|| (ErrorKind::Database, "Failed to store metadata"))
        })?;

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
