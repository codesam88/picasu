use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::sanitize::sanitize_tag;
use crate::process::sidecar_edit::{EditedItem, commit_metadata_edits};
use crate::process::transitor::{compose_by_asset_id, index_to_asset_id, store_metadata_record};
use crate::router::auth::GuardMutate;
use crate::router::auth::GuardUser;
use crate::router::{AppResult, GuardResult};
use crate::storage::cache::SnapshotReadError;
use crate::storage::db::TagInfo;
use crate::storage::db::{TREE, open_tree_snapshot_table};
use crate::storage::files::write_gate;
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

/// Add and remove tags on the listed assets and return the resulting tag list.
///
/// `indexArray` addresses the assets in the snapshot named by `timestamp`, and
/// additions and removals are applied per asset in list order after
/// NFC-normalization and filtering to valid XML characters. Each asset's
/// `.albuminfo.xmp` sidecar is rewritten on a best-effort basis, and the
/// returned list counts every tag over the whole metadata table, not only the
/// edited assets.
///
/// Corner cases: a tag that sanitizes to empty is skipped on add but still
/// serves as the removal key, and an index whose asset record no longer exists
/// is skipped and still answers 200.
///
/// Errors: 400 malformed body or unknown snapshot id — 401 missing or invalid
/// credentials; share tokens are not accepted — 405 read-only mode — 500
/// out-of-range index or storage failure.
#[utoipa::path(
        tag = "assets",
        request_body = EditTagsData,
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
            (status = 200, description = "Tags updated", body = Vec<TagInfo>),
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
#[put("/put/edit_tag", format = "json", data = "<json_data>")]
pub async fn edit_tag(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    json_data: Json<EditTagsData>,
) -> AppResult<Json<Vec<TagInfo>>> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = crate::router::put::users_password::caller_identity(&claims);

    if json_data.timestamp < 0 {
        return Err(AppError::new(ErrorKind::InvalidInput, "Invalid timestamp"));
    }

    let vec_caller = caller.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let tree_snapshot = open_tree_snapshot_table(json_data.timestamp)
            .map_err(SnapshotReadError::into_app_error)?;

        let mut edited: Vec<EditedItem> = Vec::new();

        for &index in &json_data.index_array {
            if index > usize::MAX / 2 {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Index out of bounds",
                ));
            }
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
                if let Some(path) = abstract_data.visibility_path() {
                    write_gate(&vec_caller, Some(&path), None).map_err(|_| {
                        AppError::new(ErrorKind::PermissionDenied, "Asset not writable")
                    })?;
                }
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

                edited.push(EditedItem {
                    asset_id,
                    data: abstract_data,
                });
            }
        }

        // Sidecars first, payloads second: an edit whose sidecar did not land
        // is not an edit, so it must not reach the cache.
        commit_metadata_edits(&edited, |asset_id, data: &AbstractData| {
            // Store the metadata-only payloads; identity fields are not written.
            store_metadata_record(asset_id, data, None)
                .or_raise(|| (ErrorKind::Database, "Failed to store metadata"))
        })?;

        // Caller-filtered census is read after the in-memory tree rebuild
        // below, so the just-written tags are included.
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

    Ok(Json(TREE.read_tags(&caller)))
}
