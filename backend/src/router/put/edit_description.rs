use crate::process::sanitize::sanitize_text;
use crate::process::transitor::index_to_hash;
use crate::process::xmp_write::write_sidecar_for;
use crate::storage::db::{open_data_table, open_tree_snapshot_table};

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
        request_body = SetUserDefinedDescription,
        responses(
            (status = 200, description = "Description updated"),
            (status = 400, description = "Invalid input"),
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
        let data_table = open_data_table();
        let tree_snapshot = open_tree_snapshot_table(set_user_defined_description.timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot"))?;

        let hash =
            index_to_hash(&tree_snapshot, set_user_defined_description.index).or_raise(|| {
                (
                    ErrorKind::Database,
                    format!(
                        "Failed to get hash for index {}",
                        set_user_defined_description.index
                    ),
                )
            })?;

        if let Some(guard) = data_table
            .get(&*hash)
            .or_raise(|| (ErrorKind::Database, "Failed to get data from table"))?
        {
            let mut abstract_data = guard.value();

            let description = set_user_defined_description
                .description
                .as_deref()
                .map(sanitize_text);
            abstract_data.set_description(description);

            if let Err(e) = write_sidecar_for(&abstract_data) {
                warn!("Failed to write XMP sidecar: {e}");
            }

            BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(vec![abstract_data]));
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
