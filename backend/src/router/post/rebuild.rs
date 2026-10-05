// src/router/post/rebuild.rs

use rocket::post;
use rocket::serde::json::Json;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::openapi_components::Unauthorized;
use crate::process::rebuild::{RebuildStats, rebuild_from_filesystem};
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::files::get_resolved_image_home;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;

/// Rebuild the asset tables and metadata cache from the filesystem under `IMAGE_HOME`.
///
/// Clears `ASSET_BY_PATH`, `ASSET_BY_ID`, `DUPE_INDEX` and `METADATA_TABLE`,
/// walks the image root, and repopulates all four: identity from the walk,
/// metadata through the same pipeline the incremental indexer runs. The
/// response carries the rebuild stats and is returned only after an
/// in-memory tree refresh, so it does not race a following prefetch or
/// get-data call.
///
/// Corner cases: Rows are rebuilt rather than merged: the rebuild assigns
/// new `asset_id`s, so rows keyed by the previous ids must not remain. A
/// file whose metadata pipeline fails keeps its identity, logs the failure,
/// and does not stop the walk.
///
/// Errors: 400 malformed request — 401 missing or invalid credentials —
/// 405 read-only mode — 500 `imagePath` unset, or a failure while walking the
/// image root or writing the tables.
#[utoipa::path(
        tag = "index",
        responses(
            (status = 200, description = "Rebuild complete", body = RebuildStats),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[post("/post/rebuild")]
pub async fn rebuild_handler(
    _auth: GuardAuth,
    read_only: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Json<RebuildStats>> {
    let _ = read_only?;

    let image_root = get_resolved_image_home()
        .ok_or_else(|| AppError::new(ErrorKind::Internal, "IMAGE_HOME is not configured"))?;

    // Filesystem walk plus metadata reads and thumbnail writes per media file:
    // blocking work, off the async runtime.
    let stats = tokio::task::spawn_blocking(move || rebuild_from_filesystem(&image_root))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join rebuild task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute update tree task"))?;

    Ok(Json(stats))
}
