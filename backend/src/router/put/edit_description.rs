use crate::openapi_components::Unauthorized;
use crate::process::sanitize::sanitize_text;
use crate::process::sidecar_edit::{EditedItem, commit_metadata_edits};
use crate::process::transitor::{compose_by_asset_id, index_to_asset_id, store_metadata_record};
use crate::storage::cache::SnapshotReadError;
use crate::storage::db::open_tree_snapshot_table;
use crate::storage::files::write_gate;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::router::auth::GuardMutate;
use crate::router::auth::GuardUser;
use crate::router::{AppResult, GuardResult};
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use anyhow::Result;
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

/// Set or clear the user-defined description of an asset.
///
/// `index` addresses the asset in the snapshot named by `timestamp`, and
/// `description` replaces the field: `null` clears it, other text is
/// NFC-normalized and filtered to valid XML characters. The value is written to
/// the asset's `.albuminfo.xmp` sidecar on a best-effort basis, persisted, and
/// the in-memory tree is rebuilt before the call returns. Share credentials
/// (`x-album-id` plus `x-share-id`, or `albumId` plus `shareId`) are accepted
/// alongside the admin JWT cookie.
///
/// Corner cases: an index whose asset record no longer exists is skipped
/// and still answers 200, and a failed sidecar write is logged rather than
/// failing the request.
///
/// Errors: 400 malformed body or unknown snapshot id — 401 missing or invalid
/// session — 403 no enrolled user — 405 read-only mode — 500 out-of-range
/// index or storage failure.
#[utoipa::path(
        tag = "albums",
        request_body = SetUserDefinedDescription,
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser", "GuardMutate"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["write"],
            "invariants": ["AUTH-12"]
        }))),
        responses(
            (status = 200, description = "Description updated"),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put(
    "/put/set_user_defined_description",
    data = "<set_user_defined_description>"
)]
pub async fn set_user_defined_description(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    set_user_defined_description: Json<SetUserDefinedDescription>,
) -> AppResult<()> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = crate::router::put::users_password::caller_identity(&claims);

    if set_user_defined_description.timestamp < 0 {
        return Err(AppError::new(ErrorKind::InvalidInput, "Invalid timestamp"));
    }
    if set_user_defined_description.index > usize::MAX / 2 {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "Index out of bounds",
        ));
    }

    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let tree_snapshot = open_tree_snapshot_table(set_user_defined_description.timestamp)
            .map_err(SnapshotReadError::into_app_error)?;

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
            if let Some(path) = abstract_data.visibility_path() {
                write_gate(&caller, Some(&path), None).map_err(|_| {
                    AppError::new(ErrorKind::PermissionDenied, "Asset not writable")
                })?;
            }
            let description = set_user_defined_description
                .description
                .as_deref()
                .map(sanitize_text);
            abstract_data.set_description(description);

            // Sidecar first, payload second: a description that reached only
            // the cache would be reverted by the next reindex, silently.
            let edited = [EditedItem {
                asset_id,
                data: abstract_data,
            }];
            commit_metadata_edits(&edited, |asset_id, data| {
                store_metadata_record(asset_id, data, None)
                    .or_raise(|| (ErrorKind::Database, "Failed to store metadata"))
            })?;
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
