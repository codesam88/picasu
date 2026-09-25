use crate::openapi_components::Unauthorized;
use crate::process::sanitize::sanitize_text;
use crate::process::transitor::{compose_by_asset_id, index_to_asset_id, store_metadata_record};
use crate::process::xmp_write::write_sidecar_for;
use crate::storage::db::open_tree_snapshot_table;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::router::auth::GuardReadOnlyMode;
use crate::router::auth::GuardShare;
use crate::router::{AppResult, GuardResult};
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
use log::warn;
use rocket::serde::{Deserialize, json::Json};
use serde::Serialize;

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct SetUserDefinedDescription {
    pub index: usize,
    pub description: Option<String>,
    pub timestamp: i64,
}

#[utoipa::path(
        put,
        path = "/put/set_user_defined_description",
        tag = "albums",
        request_body = SetUserDefinedDescription,
        responses(
            (status = 200, description = "Description updated"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[put(
    "/put/set_user_defined_description",
    data = "<set_user_defined_description>"
)]
pub async fn set_user_defined_description(
    auth: GuardResult<GuardShare>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    set_user_defined_description: Json<SetUserDefinedDescription>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let tree_snapshot = open_tree_snapshot_table(set_user_defined_description.timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

        let asset_id = index_to_asset_id(&tree_snapshot, set_user_defined_description.index)
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    format!(
                        "Failed to get asset_id for index {}",
                        set_user_defined_description.index
                    ),
                )
            })?;

        if let Some(mut abstract_data) = compose_by_asset_id(&asset_id)
            .or_raise(|| (ErrorKind::Database, "Failed to get data from table"))?
        {
            let description = set_user_defined_description
                .description
                .as_deref()
                .map(sanitize_text);
            abstract_data.set_description(description);

            if let Err(e) = write_sidecar_for(&abstract_data) {
                warn!("Failed to write XMP sidecar: {e}");
            }

            store_metadata_record(&asset_id, &abstract_data, None)
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

    Ok(())
}
