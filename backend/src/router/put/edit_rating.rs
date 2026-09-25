use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::transitor::{compose_by_asset_id, index_to_asset_id, store_metadata_record};
use crate::process::xmp_write::write_sidecar_for;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::open_tree_snapshot_table;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use arrayvec::ArrayString;
use log::warn;
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
        tag = "assets",
        request_body = EditRatingData,
        responses(
            (status = 200, description = "Rating updated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
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

        let mut data_to_store: Vec<(ArrayString<64>, AbstractData)> = Vec::new();

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
                if let Err(e) = write_sidecar_for(&abstract_data) {
                    warn!("Failed to write XMP sidecar: {e}");
                }
                data_to_store.push((asset_id, abstract_data));
            }
        }

        for (asset_id, data) in &data_to_store {
            store_metadata_record(asset_id, data, None)
                .or_raise(|| (ErrorKind::Database, "Failed to store metadata"))?;
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
