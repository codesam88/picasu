use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::process::transitor::index_to_hash;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::TagInfo;
use crate::storage::db::{open_data_table, open_tree_snapshot_table};
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
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
        request_body = EditTagsData,
        responses(
            (status = 200, description = "Tags updated", body = Vec<TagInfo>),
            (status = 400, description = "Invalid input"),
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

                // Apply tag additions and removals (only regular tags)
                let tags = abstract_data.tag_mut();
                for tag in &json_data.add_tags_array {
                    tags.insert(tag.clone());
                }
                for tag in &json_data.remove_tags_array {
                    tags.remove(tag);
                }

                data_to_flush.push(abstract_data);
            }
        }

        // Flush data
        if !data_to_flush.is_empty() {
            BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(data_to_flush));
        }

        // Return TagInfo
        crate::storage::cache::TreeSnapshot::read_tags().map_err(AppError::from)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    // Wait for the in-memory Tree to be updated
    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;

    Ok(Json(vec_tags_info))
}
