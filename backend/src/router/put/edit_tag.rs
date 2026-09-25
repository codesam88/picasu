use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::sanitize::sanitize_tag;
use crate::process::transitor::{compose_by_asset_id, index_to_asset_id, store_metadata_record};
use crate::process::xmp_write::write_sidecar_for;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::TagInfo;
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
pub struct EditTagsData {
    index_array: Vec<usize>,
    add_tags_array: Vec<String>,
    remove_tags_array: Vec<String>,
    timestamp: i64,
}

#[utoipa::path(
        put,
        path = "/put/edit_tag",
        tag = "assets",
        request_body = EditTagsData,
        responses(
            (status = 200, description = "Tags updated", body = Vec<TagInfo>),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[put("/put/edit_tag", format = "json", data = "<json_data>")]
pub async fn edit_tag(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<EditTagsData>,
) -> AppResult<Json<Vec<TagInfo>>> {
    let _ = auth?;
    let _ = read_only_mode?;

    let vec_tags_info = tokio::task::spawn_blocking(move || -> Result<Vec<TagInfo>, AppError> {
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

            // Read the composed view (identity from AssetRecord, metadata
            // from the stored payload), mutate metadata only, then extract
            // the payload back — identity is never written from this view.
            if let Some(mut abstract_data) = compose_by_asset_id(&asset_id)
                .or_raise(|| (ErrorKind::Database, "Failed to get data"))?
            {
                // Apply tag additions and removals (only regular tags)
                let tags = abstract_data.tag_mut();
                for tag in &json_data.add_tags_array {
                    let clean = sanitize_tag(tag);
                    if !clean.is_empty() {
                        tags.insert(clean);
                    }
                }
                for tag in &json_data.remove_tags_array {
                    tags.remove(&sanitize_tag(tag));
                }

                if let Err(e) = write_sidecar_for(&abstract_data) {
                    warn!("Failed to write XMP sidecar: {e}");
                }
                data_to_store.push((asset_id, abstract_data));
            }
        }

        // Store the metadata-only payloads; identity fields are not written.
        for (asset_id, data) in &data_to_store {
            store_metadata_record(asset_id, data, None)
                .or_raise(|| (ErrorKind::Database, "Failed to store metadata"))?;
        }

        // Return TagInfo
        crate::storage::cache::TreeSnapshot::read_tags().map_err(AppError::from)
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

    Ok(Json(vec_tags_info))
}
